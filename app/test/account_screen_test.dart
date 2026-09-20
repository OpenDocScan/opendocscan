import 'package:docscan/account/account_api.dart';
import 'package:docscan/account/account_controller.dart';
import 'package:docscan/account/account_screen.dart';
import 'package:docscan/account/session.dart';
import 'package:docscan/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'fakes.dart';

void main() {
  (int, Object?) server(String route) => switch (route) {
    'GET /v1/credits/balance' => (200, {'balance': 4200}),
    'GET /v1/credits/history' => (
      200,
      {
        'entries': [
          {
            'kind': 'debit',
            'amount': -40,
            'created_at': 1757500000,
            'app_name': 'OpenCapture',
            'ref_id': 'transcribe',
          },
        ],
        'next_cursor': null,
      },
    ),
    'GET /v1/payments/packages' => (
      200,
      {
        'packages': [
          {'id': 'starter', 'credits': 1000, 'usd_price': 500},
        ],
        'rails': {'stripe': true},
      },
    ),
    _ => (404, null),
  };

  Future<AccountController> mount(
    WidgetTester tester, {
    Session? session,
    FakeBrowser? browser,
  }) async {
    final fake = FakeServer(server);
    final controller = AccountController(
      api: AccountApi(
        client: fake.client,
        store: MemorySessionStore(session),
      ),
      browser: browser ?? FakeBrowser(),
    );
    await controller.start();
    await tester.pumpWidget(
      MaterialApp(
        theme: buildTheme(Brightness.dark),
        home: AccountScreen(controller: controller),
      ),
    );
    await tester.pumpAndSettle();
    return controller;
  }

  /// Everything a person can actually read on the screen.
  ///
  /// The web version of this check has to walk shadow roots, because the string
  /// that bit there lived inside a vendored element rather than in any file the
  /// app wrote. Here every string is ours, which makes the same assertion cheap
  /// and no less worth making.
  List<String> visibleText(WidgetTester tester) => tester
      .widgetList<Text>(find.byType(Text))
      .map((text) => text.data ?? text.textSpan?.toPlainText() ?? '')
      .toList();

  testWidgets('signed out, it offers sign-in and shows no balance',
      (tester) async {
    await mount(tester);

    expect(find.byKey(const Key('sign-in')), findsOneWidget);
    expect(find.text('Sign in to OpenDocScan'), findsOneWidget);

    // A balance of 0 drawn while signed out reads as a real balance of zero,
    // which is a different and much worse thing to tell somebody.
    expect(find.byKey(const Key('balance')), findsNothing);
    expect(find.byKey(const Key('balance-unknown')), findsNothing);
  });

  testWidgets('it says an account is optional, because it is', (tester) async {
    await mount(tester);

    final text = visibleText(tester).join(' ');
    expect(text, contains('do not need it'));
    expect(text, contains('signed out'));
  });

  testWidgets('the panel carries the product glyph, not a placeholder letter',
      (tester) async {
    await mount(tester);

    // OpenPixels shipped mark="P" and it read as a placeholder, because a
    // single letter in a box is exactly what one looks like. The glyph here is
    // the one the app icon is drawn from, and the same one the web panel uses.
    expect(find.text('▤'), findsOneWidget);
    final letters = visibleText(tester).where((s) => RegExp(r'^[A-Za-z]$').hasMatch(s));
    expect(letters, isEmpty);
  });

  testWidgets('nothing a user can read names the platform', (tester) async {
    // The suite is shared plumbing, not a second company to introduce to
    // somebody who installed a document scanner.
    await mount(tester, session: fakeSession);

    for (final line in visibleText(tester)) {
      expect(line.toLowerCase(), isNot(contains('openapps')), reason: line);
    }
  });

  testWidgets('signed in, the balance and the ledger are the screen',
      (tester) async {
    await mount(tester, session: fakeSession);

    expect(find.byKey(const Key('balance')), findsOneWidget);
    expect(find.text('4200'), findsOneWidget);
    // Not scoped to this app: the account is shared, so a ledger filtered to
    // OpenDocScan would leave a balance that dropped for unnamed reasons.
    expect(find.text('OpenCapture · transcribe'), findsOneWidget);
    expect(find.byKey(const Key('sign-in')), findsNothing);
  });

  testWidgets('buying opens the browser rather than a window in the app',
      (tester) async {
    // Stripe cannot redirect back into an app, and a checkout inside a webview
    // is a card number typed into something this app could read.
    final browser = FakeBrowser();
    await mount(tester, session: fakeSession, browser: browser);

    expect(find.byKey(const Key('buy-starter')), findsOneWidget);
    expect(find.text(r'$5.00'), findsOneWidget);
  });

  testWidgets('signing out returns to the panel', (tester) async {
    final controller = await mount(tester, session: fakeSession);
    expect(find.byKey(const Key('balance')), findsOneWidget);

    await controller.signOut();
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('sign-in')), findsOneWidget);
    expect(find.byKey(const Key('balance')), findsNothing);
  });
}
