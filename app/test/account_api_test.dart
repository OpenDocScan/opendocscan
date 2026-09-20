import 'package:docscan/account/account_api.dart';
import 'package:docscan/account/openapps.dart';
import 'package:docscan/account/session.dart';
import 'package:flutter_test/flutter_test.dart';

import 'fakes.dart';

/// The client, at the door.
///
/// The Android release build now carries `INTERNET`, which it deliberately did
/// not before accounts existed. What replaced that guarantee is the host check
/// in this class, so the first test here is the one that matters most: it is
/// the whole of the privacy claim on Android now, and it is worth more than the
/// comment explaining it.
void main() {
  test('a request to any host but the account server never leaves', () async {
    // The check reads the constant, so the only way to exercise it is to point
    // a URL somewhere else and watch the door hold. `isAccountHost` is public
    // for exactly this reason.
    expect(isAccountHost(Uri.parse('$kAuthBase/v1/me')), isTrue);

    expect(isAccountHost(Uri.parse('https://example.test/v1/me')), isFalse);
    expect(isAccountHost(Uri.parse('http://auth.opendocscan.com/v1/me')), isFalse,
        reason: 'plain http is not the account host');
    expect(isAccountHost(Uri.parse('https://auth.opendocscan.com.evil.test/')),
        isFalse,
        reason: 'a suffix match would accept an attacker-registered domain');
    expect(isAccountHost(Uri.parse('https://gateway.opendocscan.com/')), isFalse,
        reason: 'nothing in this app calls the gateway, so it is not allowed');
  });

  test('signing in asks for the code to come back with no fragment', () {
    final api = FakeServer((_) => (200, null)).api();
    final url = api.signInUrl();

    expect(url.host, Uri.parse(kAuthBase).host);
    expect(url.path, '/v1/auth/oidc/google/start');

    final returnTo = Uri.parse(url.queryParameters['return_to']!);
    // The server refuses a return_to carrying a fragment outright — "return_to
    // must not contain a fragment" — and the failure is a 400 nothing in the
    // app's UI would ever surface.
    expect(returnTo.hasFragment, isFalse);
    expect(returnTo.scheme, 'https');
  });

  test('a one-time code becomes a stored session', () async {
    final server = FakeServer(
      (route) => switch (route) {
        'POST /v1/auth/oidc/exchange' => (
          200,
          {'access_token': 'a', 'refresh_token': 'r'},
        ),
        _ => (404, null),
      },
    );
    final store = MemorySessionStore();
    final api = AccountApi(client: server.client, store: store);

    final session = await api.exchange('the-code');

    expect(session.accessToken, 'a');
    expect(await store.read(), session, reason: 'it must survive a relaunch');
  });

  test('an expired access token is refreshed once, and the call replayed',
      () async {
    var balanceCalls = 0;
    final server = FakeServer(
      (route) => switch (route) {
        '/v1/credits/balance' => (200, null),
        'GET /v1/credits/balance' => ++balanceCalls == 1
            ? (401, null)
            : (200, {'balance': 7}),
        'POST /v1/auth/refresh' => (
          200,
          {'access_token': 'a2', 'refresh_token': 'r2'},
        ),
        _ => (404, null),
      },
    );
    final store = MemorySessionStore(fakeSession);
    final api = AccountApi(client: server.client, store: store);
    await api.load();

    expect(await api.balance(), 7);
    expect(server.routes, [
      'GET /v1/credits/balance',
      'POST /v1/auth/refresh',
      'GET /v1/credits/balance',
    ]);
    expect((await store.read())!.accessToken, 'a2',
        reason: 'the rotated pair has to be kept or the next launch is signed out');
  });

  test('a refresh that fails signs the session out rather than looping',
      () async {
    final server = FakeServer((route) => (401, null));
    final store = MemorySessionStore(fakeSession);
    final api = AccountApi(client: server.client, store: store);
    await api.load();

    await expectLater(
      api.balance(),
      throwsA(isA<AccountError>().having((e) => e.isUnauthorized, 'is 401', true)),
    );
    expect(await store.read(), isNull);
    // One balance, one refresh, and no second balance: a retry loop here would
    // hammer the server with a dead token.
    expect(server.routes, ['GET /v1/credits/balance', 'POST /v1/auth/refresh']);
  });

  test('a dead connection is reported as one, not as a server error', () async {
    final api = AccountApi(
      client: ThrowingClient(),
      store: MemorySessionStore(),
    );

    await expectLater(
      api.exchange('x'),
      throwsA(isA<AccountError>().having((e) => e.code, 'code', 'network')),
    );
  });

  test('the ledger is read the way the web reads it', () async {
    final server = FakeServer(
      (route) => (200, {
        'entries': [
          {
            'kind': 'debit',
            'amount': -40,
            'created_at': 1757500000,
            'app_name': 'OpenCapture',
            'ref_id': 'transcribe',
          },
          {'kind': 'topup', 'amount': 1000, 'created_at': 1757400000},
        ],
        'next_cursor': 'page-2',
      }),
    );
    final api = AccountApi(
      client: server.client,
      store: MemorySessionStore(fakeSession),
    );
    await api.load();

    final history = await api.history();

    expect(history.complete, isFalse);
    // Same wording as <openapps-history>, so one account does not read as two.
    expect(history.entries.first.label, 'OpenCapture · transcribe');
    expect(history.entries.last.label, 'Credits purchased');
  });

  test('every rail off means nothing to buy, which is a server state', () async {
    final server = FakeServer(
      (route) => (200, {
        'packages': [
          {'id': 'starter', 'credits': 1000, 'usd_price': 500},
        ],
        'rails': {'stripe': false, 'ethereum': false, 'lightning': false},
      }),
    );
    final packages = await anonymousApi(server).packages();

    expect(packages.packages.single.price, r'$5.00');
    expect(packages.canBuy, isFalse);
  });

  test('signing out clears the device even when the server cannot be told',
      () async {
    // Somebody tapping sign out on a train has to end up signed out, or the
    // control is a lie.
    final store = MemorySessionStore(fakeSession);
    final api = AccountApi(client: ThrowingClient(), store: store);
    await api.load();

    await api.signOut();

    expect(api.isSignedIn, isFalse);
    expect(await store.read(), isNull);
  });

  test('a rejected sign-in code does not claim a session expired', () async {
    // Found on an emulator, not here: the server answers a bad one-time code
    // with a plain 401, the client turned every 401 into "the session has
    // expired", and a person who had never signed in read that on screen.
    final server = FakeServer(
      (route) => (401, {
        'error': {'code': 'unauthorized', 'message': 'missing or invalid credentials'},
      }),
    );
    final store = MemorySessionStore();
    final api = AccountApi(client: server.client, store: store);

    await expectLater(
      api.exchange('already-used'),
      throwsA(
        isA<AccountError>()
            .having((e) => e.message, 'message', contains('already have been used'))
            .having((e) => e.message, 'message', isNot(contains('session has expired'))),
      ),
    );
    // And nothing was signed out, because nothing was signed in. The old code
    // wrote a null session here on every failed attempt.
    expect(store.writes, 0);
  });

  test('a 401 on an authenticated call still ends the session', () async {
    // The other half of the same branch, so narrowing it cannot quietly turn
    // an expired token into an error banner nobody can clear.
    final server = FakeServer((route) => (401, null));
    final store = MemorySessionStore(fakeSession);
    final api = AccountApi(client: server.client, store: store);
    await api.load();

    await expectLater(api.balance(), throwsA(isA<AccountError>()));
    expect(await store.read(), isNull);
  });
}
