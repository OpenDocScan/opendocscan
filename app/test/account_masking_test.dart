import 'dart:io';

import 'package:docscan/account/openapps.dart';
import 'package:flutter_test/flutter_test.dart';

/// The rules that a refactor breaks silently, checked against the source.
///
/// None of these can fail in a way a person would notice: a second copy of a
/// hostname keeps working until the day the domain moves, and a callback scheme
/// that drifts from the manifest fails only on a real device, only after a real
/// Google sign-in, and with no error anywhere.
void main() {
  final lib = Directory('lib');
  final dartFiles = lib
      .listSync(recursive: true)
      .whereType<File>()
      .where((file) => file.path.endsWith('.dart'))
      // Generated bridge code is not ours to hold to this.
      .where((file) => !file.path.contains('/src/rust/'))
      .toList();

  test('the platform is named nowhere a user or a maintainer would meet it', () {
    final offenders = [
      for (final file in dartFiles)
        if (file.readAsStringSync().contains('openapps.network')) file.path,
    ];
    expect(offenders, isEmpty,
        reason: 'someone who installed a document scanner has never heard of it');
  });

  test('each account hostname is written down exactly once', () {
    // OpenCapture kept its base URL as a literal in two places, moved to its
    // own domain, fixed one, and shipped the other pointing at the platform's
    // host for months. This is that bug, as an assertion.
    for (final host in ['auth.opendocscan.com', 'gateway.opendocscan.com']) {
      final holders = [
        for (final file in dartFiles)
          if (file.readAsStringSync().contains(host)) file.path,
      ];
      expect(holders, ['lib/account/openapps.dart'], reason: host);
    }
  });

  test('the callback scheme in the constants is the one the OS is told about',
      () {
    // The single most expensive drift available here: change the constant, and
    // sign-in still opens, still authenticates, and simply never comes back.
    final manifest =
        File('android/app/src/main/AndroidManifest.xml').readAsStringSync();
    expect(manifest, contains('android:scheme="$kCallbackScheme"'));
    expect(manifest, contains('android:host="$kCallbackHost"'));

    final plist = File('ios/Runner/Info.plist').readAsStringSync();
    expect(plist, contains('<string>$kCallbackScheme</string>'));
  });

  test('Android can see a browser, and is allowed to reach the network', () {
    final manifest =
        File('android/app/src/main/AndroidManifest.xml').readAsStringSync();

    // Android 11 hides every other installed package unless it is declared.
    // Without this, sign-in fails with "Could not open a browser" on a phone
    // that plainly has one.
    expect(manifest, contains('android:scheme="https"'),
        reason: 'the <queries> entry url_launcher needs');
    expect(manifest, contains('android.permission.INTERNET'));
  });

  test('the callback comes back to an origin the server already allows', () {
    // Not the deep link itself: that origin is valid but unlisted, and adding
    // it restarts a container shared by every product in the suite. This page
    // is on the list already — measured, 307 — and forwards the code on.
    final returnTo = Uri.parse(kSignInReturnTo);
    expect(returnTo.scheme, 'https');
    expect(returnTo.host, 'opendocscan.com');
    expect(returnTo.hasFragment, isFalse,
        reason: 'the server refuses a return_to carrying a fragment');

    // And the page it lands on has to be the one that does the forwarding.
    final page = File('../apps/web/account.html').readAsStringSync();
    expect(page, contains('$kCallbackScheme://$kCallbackHost'));
    expect(page, contains(returnTo.queryParameters.keys.single));
  });
}
