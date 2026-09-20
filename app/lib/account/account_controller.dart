import 'dart:async';

import 'package:flutter/foundation.dart';

import 'account_api.dart';
import 'sign_in.dart';

/// Where the account is, from the app's point of view.
enum AccountStage {
  /// The store has not been read yet. Every launch begins here, and the UI must
  /// not draw a signed-out state during it — a returning user would watch their
  /// account blink out and back in on every cold start.
  loading,

  signedOut,

  /// The browser is open and the round trip has not come back. It may never
  /// come back: someone can abandon a sign-in by switching apps, so nothing
  /// here blocks and nothing waits forever.
  signingIn,

  signedIn,
}

/// Everything the account screen needs, and nothing about how it looks.
///
/// The platform is injected in three pieces — the API, the browser, the
/// deep-link source — so the whole flow, including the return trip that is the
/// part that silently fails in production, runs in `flutter test` with no
/// device and no network.
class AccountController extends ChangeNotifier {
  AccountController({
    required this.api,
    this.browser = const SystemBrowser(),
    this.links,
  });

  final AccountApi api;
  final Browser browser;

  /// Where the OS delivers the finished sign-in. Null in tests that are not
  /// about the return trip, and in nothing else.
  final DeepLinks? links;

  StreamSubscription<Uri>? _subscription;

  AccountStage stage = AccountStage.loading;

  /// The last thing that went wrong, in the words the user should read.
  String? error;

  int? balance;
  Packages? packages;
  final List<CreditEntry> entries = [];
  String? _cursor;
  bool historyComplete = false;
  bool busy = false;

  /// Read the store, then start listening for the return trip.
  ///
  /// The order matters. The store is read *before* the deep-link subscription
  /// so that a cold start launched **by** the callback link cannot exchange a
  /// code and then have a stale read overwrite the session it just wrote.
  Future<void> start() async {
    await api.load();
    stage = api.isSignedIn ? AccountStage.signedIn : AccountStage.signedOut;
    notifyListeners();

    final links = this.links;
    if (links != null) {
      _subscription = links.stream.listen(_handleLink);
      final initial = await links.initial();
      if (initial != null) await _handleLink(initial);
    }

    if (api.isSignedIn) unawaited(refresh());
    unawaited(_loadPackages());
  }

  @override
  void dispose() {
    _subscription?.cancel();
    super.dispose();
  }

  // ------------------------------------------------------------- signing in

  Future<void> signIn() async {
    error = null;
    stage = AccountStage.signingIn;
    notifyListeners();

    final opened = await browser.open(api.signInUrl());
    if (!opened) {
      // No browser answered the intent. Rare, but it leaves the UI stuck on
      // "waiting" forever if it is not handled, which looks like the sign-in
      // itself hanging.
      stage = AccountStage.signedOut;
      error = 'Could not open a browser to sign in.';
      notifyListeners();
    }
  }

  /// A URI arrived from the OS. Anything that is not our callback is ignored
  /// rather than treated as a failure — other links may reach this app later.
  Future<void> _handleLink(Uri uri) async {
    final code = codeFrom(uri);
    if (code == null) return;
    await completeSignIn(code);
  }

  @visibleForTesting
  Future<void> completeSignIn(String code) async {
    stage = AccountStage.signingIn;
    error = null;
    notifyListeners();

    try {
      await api.exchange(code);
      stage = AccountStage.signedIn;
      notifyListeners();
      await refresh();
    } on AccountError catch (failure) {
      stage = AccountStage.signedOut;
      error = failure.message;
      notifyListeners();
    }
  }

  Future<void> signOut() async {
    await api.signOut();
    stage = AccountStage.signedOut;
    balance = null;
    entries.clear();
    _cursor = null;
    historyComplete = false;
    error = null;
    notifyListeners();
  }

  // ----------------------------------------------------------------- reading

  /// Balance and the first page of history, together — they are one screen.
  Future<void> refresh() async {
    if (!api.isSignedIn) return;
    busy = true;
    notifyListeners();

    try {
      balance = await api.balance();
      entries.clear();
      _cursor = null;
      historyComplete = false;
      await _loadHistory();
      error = null;
    } on AccountError catch (failure) {
      if (failure.isUnauthorized) {
        await signOut();
        return;
      }
      error = failure.message;
    } finally {
      busy = false;
      notifyListeners();
    }
  }

  Future<void> loadMore() async {
    if (historyComplete || busy) return;
    busy = true;
    notifyListeners();
    try {
      await _loadHistory();
    } on AccountError catch (failure) {
      error = failure.message;
    } finally {
      busy = false;
      notifyListeners();
    }
  }

  Future<void> _loadHistory() async {
    final page = await api.history(cursor: _cursor);
    entries.addAll(page.entries);
    _cursor = page.nextCursor;
    historyComplete = page.complete;
  }

  Future<void> _loadPackages() async {
    try {
      packages = await api.packages();
    } on AccountError {
      // Nothing to buy is a state the screen already draws. A failure to
      // *ask* should not put an error banner over a working account.
      packages = null;
    }
    notifyListeners();
  }

  // ------------------------------------------------------------------ buying

  Future<void> buy(CreditPackage package) async {
    error = null;
    notifyListeners();
    try {
      final url = await api.checkout(package.id);
      await browser.open(url);
    } on AccountError catch (failure) {
      error = failure.message;
      notifyListeners();
    }
  }
}
