import 'package:docscan/account/account_api.dart';
import 'package:docscan/account/account_controller.dart';
import 'package:docscan/account/openapps.dart';
import 'package:docscan/account/session.dart';
import 'package:docscan/account/sign_in.dart';
import 'package:flutter_test/flutter_test.dart';

import 'fakes.dart';

/// The return trip, which is where sign-in dies quietly.
///
/// Everything up to the browser is easy to see working: a button, a URL, a
/// Google page. What fails without a word is the journey back — the browser
/// hands the code to the operating system, the OS wakes the app, and if nothing
/// is listening at that moment the session is simply never created. There is no
/// error, no log line, and a user who tries again gets the same nothing.
///
/// So these tests are all about what happens after the browser.
void main() {
  (int, Object?) signedInServer(String route) => switch (route) {
    'POST /v1/auth/oidc/exchange' => (
      200,
      {'access_token': 'a', 'refresh_token': 'r'},
    ),
    'GET /v1/credits/balance' => (200, {'balance': 120}),
    'GET /v1/credits/history' => (200, {'entries': [], 'next_cursor': null}),
    'GET /v1/payments/packages' => (200, {'packages': [], 'rails': {}}),
    _ => (404, null),
  };

  test('a cold start launched by the callback signs in', () async {
    // The common case, and the one a listener attached from the account
    // screen's initState would miss entirely: signing in leaves the app, and
    // Android is free to have killed it by the time the browser comes back.
    final server = FakeServer(signedInServer);
    final links = FakeDeepLinks(
      cold: Uri.parse('opendocscan://auth#code=one-time'),
    );
    final controller = AccountController(
      api: server.api(),
      browser: FakeBrowser(),
      links: links,
    );

    await controller.start();
    await pumpEventQueue();

    expect(controller.stage, AccountStage.signedIn);
    expect(server.routes, contains('POST /v1/auth/oidc/exchange'));
    expect(controller.balance, 120);
    await links.close();
  });

  test('a link arriving while the app runs signs in too', () async {
    final server = FakeServer(signedInServer);
    final links = FakeDeepLinks();
    final controller = AccountController(
      api: server.api(),
      browser: FakeBrowser(),
      links: links,
    );
    await controller.start();
    expect(controller.stage, AccountStage.signedOut);

    links.arrive(Uri.parse('opendocscan://auth#code=one-time'));
    await pumpEventQueue();

    expect(controller.stage, AccountStage.signedIn);
    await links.close();
  });

  test('the code is read from the fragment, which is where it arrives', () {
    // The account server's own sign-in page says it in as many words: "No
    // fragment: the callback puts the one-time code there." A reader that
    // only checked the query string would find nothing, every time, silently.
    expect(codeFrom(Uri.parse('opendocscan://auth#code=abc')), 'abc');
    expect(codeFrom(Uri.parse('opendocscan://auth#state=x&code=abc')), 'abc');

    // And from the query as well, so a future server that moves it still works.
    expect(codeFrom(Uri.parse('opendocscan://auth?code=abc')), 'abc');
  });

  test('links that are not the callback are left alone', () async {
    // This app may one day be opened by a link to a document. Treating an
    // unrecognised URI as a failed sign-in would put an error on the screen
    // for something that has nothing to do with the account.
    expect(codeFrom(Uri.parse('opendocscan://open?file=1')), isNull);
    expect(codeFrom(Uri.parse('https://opendocscan.com/account#code=abc')), isNull);
    expect(codeFrom(Uri.parse('opendocscan://auth')), isNull);

    final server = FakeServer(signedInServer);
    final links = FakeDeepLinks();
    final controller = AccountController(
      api: server.api(),
      browser: FakeBrowser(),
      links: links,
    );
    await controller.start();

    links.arrive(Uri.parse('opendocscan://open?file=1'));
    await pumpEventQueue();

    expect(controller.stage, AccountStage.signedOut);
    expect(controller.error, isNull);
    expect(server.routes, isNot(contains('POST /v1/auth/oidc/exchange')));
    await links.close();
  });

  test('signing in opens the system browser at the account server', () async {
    final browser = FakeBrowser();
    final controller = AccountController(
      api: FakeServer(signedInServer).api(),
      browser: browser,
    );
    await controller.start();

    await controller.signIn();

    expect(controller.stage, AccountStage.signingIn);
    final opened = browser.opened.single;
    expect(opened.host, Uri.parse(kAuthBase).host);
    expect(opened.queryParameters['return_to'], kSignInReturnTo);
  });

  test('a phone with no browser says so instead of waiting forever', () async {
    // Rare, but if it is not handled the panel sits on "finish in your
    // browser" for a browser that never opened, which reads as the sign-in
    // itself having hung.
    final controller = AccountController(
      api: FakeServer(signedInServer).api(),
      browser: FakeBrowser(succeeds: false),
    );
    await controller.start();

    await controller.signIn();

    expect(controller.stage, AccountStage.signedOut);
    expect(controller.error, contains('browser'));
  });

  test('a stored session is signed in before anything is drawn', () async {
    // The stage must never pass through signedOut on the way. A returning user
    // watching their account blink out and back in on every cold start is the
    // symptom of reading the store after the first frame.
    final server = FakeServer(signedInServer);
    final controller = AccountController(
      api: AccountApi(
        client: server.client,
        store: MemorySessionStore(fakeSession),
      ),
      browser: FakeBrowser(),
    );

    final stages = <AccountStage>[];
    controller.addListener(() => stages.add(controller.stage));

    await controller.start();
    await pumpEventQueue();

    expect(stages.first, AccountStage.signedIn);
    expect(stages, isNot(contains(AccountStage.signedOut)));
  });

  test('a code the server rejects leaves a message, not a spinner', () async {
    final server = FakeServer(
      (route) => route == 'POST /v1/auth/oidc/exchange'
          ? (400, {'error': {'code': 'bad_request', 'message': 'code expired'}})
          : (200, null),
    );
    final links = FakeDeepLinks(cold: Uri.parse('opendocscan://auth#code=old'));
    final controller = AccountController(
      api: server.api(),
      browser: FakeBrowser(),
      links: links,
    );

    await controller.start();
    await pumpEventQueue();

    expect(controller.stage, AccountStage.signedOut);
    expect(controller.error, 'code expired');
    await links.close();
  });

  test('signing out forgets the balance as well as the session', () async {
    final server = FakeServer(signedInServer);
    final controller = AccountController(
      api: AccountApi(
        client: server.client,
        store: MemorySessionStore(fakeSession),
      ),
      browser: FakeBrowser(),
    );
    await controller.start();
    await pumpEventQueue();
    expect(controller.balance, 120);

    await controller.signOut();

    // A balance left behind would be drawn over the sign-in panel on the way
    // out, and it is somebody else's number the moment a second person signs in.
    expect(controller.balance, isNull);
    expect(controller.entries, isEmpty);
    expect(controller.stage, AccountStage.signedOut);
  });
}
