import 'package:flutter/material.dart';

import '../layout.dart';
import '../theme.dart';
import 'account_api.dart';
import 'account_controller.dart';

/// The account, on its own screen.
///
/// Its own screen rather than a sheet over the scanner, for the same reason the
/// web app gives it its own page: signing in leaves the app for a browser and
/// comes back through a cold start, so anything the scanner was holding — a
/// captured page, a half-built document — would be gone. Nothing on this route
/// is worth losing.
///
/// Nothing in the product is behind any of this. The account exists so that
/// credits bought in one of our apps are the same credits in the others, and
/// OpenDocScan spends none: every operation it performs runs on this phone's
/// own processor, so there is nothing to meter.
class AccountScreen extends StatefulWidget {
  const AccountScreen({super.key, required this.controller});

  final AccountController controller;

  @override
  State<AccountScreen> createState() => _AccountScreenState();
}

class _AccountScreenState extends State<AccountScreen> {
  late final AppLifecycleListener _lifecycle;

  @override
  void initState() {
    super.initState();
    // Buying happens in a browser and ends on the server's own confirmation
    // page, because Stripe cannot redirect back into an app. So the moment the
    // user returns here is the moment the balance may have changed, and it is
    // the only signal this app gets.
    _lifecycle = AppLifecycleListener(
      onResume: () => widget.controller.refresh(),
    );
  }

  @override
  void dispose() {
    _lifecycle.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Account')),
      body: SafeArea(
        child: AnimatedBuilder(
          animation: widget.controller,
          builder: (context, _) => _Body(controller: widget.controller),
        ),
      ),
    );
  }
}

class _Body extends StatelessWidget {
  const _Body({required this.controller});

  final AccountController controller;

  @override
  Widget build(BuildContext context) {
    if (controller.stage == AccountStage.loading) {
      return const Center(
        key: Key('account-loading'),
        child: CircularProgressIndicator(),
      );
    }

    return RefreshIndicator(
      onRefresh: controller.refresh,
      child: ListView(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 32),
        children: [
          Constrained(
            max: kReadableWidth,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (controller.error != null) _Error(message: controller.error!),
                if (controller.stage == AccountStage.signedIn) ...[
                  _Balance(controller: controller),
                  _Buy(controller: controller),
                  _Ledger(controller: controller),
                  const SizedBox(height: 24),
                  _SignOut(controller: controller),
                ] else
                  // Signed out, the panel is the whole screen. Each of the
                  // sections above renders its own "sign in to see this"
                  // placeholder, and stacking three of them under a card that
                  // already says it is three restatements of one sentence.
                  _SignInPanel(controller: controller),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// The sign-in card. The same shape as `<openapps-login variant="panel">` on
/// the web — mark, heading, description, then the one method this server has
/// that a phone can complete.
class _SignInPanel extends StatelessWidget {
  const _SignInPanel({required this.controller});

  final AccountController controller;

  @override
  Widget build(BuildContext context) {
    final theme = DocScanTheme.of(context);
    final waiting = controller.stage == AccountStage.signingIn;

    return Container(
      margin: const EdgeInsets.only(top: 16),
      padding: const EdgeInsets.all(24),
      decoration: BoxDecoration(
        color: theme.surface,
        border: Border.all(color: theme.hairline),
        borderRadius: BorderRadius.circular(18),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // The product's own glyph, not an initial. A single letter in a box
          // reads as a placeholder, because that is exactly what it looks
          // like — the web panel carries the same ▤ the favicon is drawn from,
          // so the app and the browser agree about what product this is.
          Align(
            alignment: Alignment.centerLeft,
            child: Container(
              width: 46,
              height: 46,
              alignment: Alignment.center,
              decoration: BoxDecoration(
                color: theme.surfaceRaised,
                border: Border.all(color: theme.hairline),
                borderRadius: BorderRadius.circular(12),
              ),
              child: Text(
                '▤',
                style: TextStyle(fontSize: 22, color: theme.accent),
              ),
            ),
          ),
          const SizedBox(height: 18),
          Text(
            'Sign in to OpenDocScan',
            style: TextStyle(
              color: theme.textStrong,
              fontSize: 21,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 8),
          Text(
            'One account across our apps. You do not need it here — nothing in '
            'OpenDocScan is behind it, and scanning works signed out.',
            style: TextStyle(
              color: theme.textMuted,
              fontSize: 14,
              height: 1.45,
            ),
          ),
          const SizedBox(height: 22),
          if (waiting)
            Column(
              key: const Key('waiting-for-browser'),
              children: [
                Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    const SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    ),
                    const SizedBox(width: 12),
                    Flexible(
                      child: Text(
                        'Finish signing in in your browser.',
                        style: TextStyle(color: theme.textMuted, fontSize: 14),
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 12),
                TextButton(
                  onPressed: controller.signIn,
                  child: const Text('Open the browser again'),
                ),
              ],
            )
          else
            FilledButton(
              key: const Key('sign-in'),
              onPressed: controller.signIn,
              child: const Text('Continue with Google'),
            ),
          const SizedBox(height: 14),
          Text(
            'Signing in opens your browser rather than a window inside this '
            'app, so your password is never typed into something OpenDocScan '
            'could read.',
            style: TextStyle(color: theme.textMuted, fontSize: 12, height: 1.4),
          ),
        ],
      ),
    );
  }
}

class _Balance extends StatelessWidget {
  const _Balance({required this.controller});

  final AccountController controller;

  @override
  Widget build(BuildContext context) {
    final theme = DocScanTheme.of(context);
    final balance = controller.balance;

    return Container(
      margin: const EdgeInsets.only(top: 16),
      padding: const EdgeInsets.all(20),
      decoration: BoxDecoration(
        color: theme.surface,
        border: Border.all(color: theme.hairline),
        borderRadius: BorderRadius.circular(18),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'Credits',
                  style: TextStyle(
                    color: theme.textMuted,
                    fontSize: 12,
                    letterSpacing: 1.1,
                  ),
                ),
                const SizedBox(height: 6),
                // A number is only shown once one has actually arrived. A `0`
                // drawn while the request is still out reads as a real balance
                // of zero, which is a different and much worse thing to tell
                // somebody who has just bought credits.
                balance == null
                    ? SizedBox(
                        height: 34,
                        child: Align(
                          alignment: Alignment.centerLeft,
                          child: Text(
                            '—',
                            key: const Key('balance-unknown'),
                            style: TextStyle(
                              color: theme.textMuted,
                              fontSize: 28,
                            ),
                          ),
                        ),
                      )
                    : Text(
                        '$balance',
                        key: const Key('balance'),
                        style: TextStyle(
                          color: theme.textStrong,
                          fontSize: 30,
                          fontWeight: FontWeight.w600,
                          fontFeatures: const [FontFeature.tabularFigures()],
                        ),
                      ),
              ],
            ),
          ),
          if (controller.busy)
            const SizedBox(
              width: 18,
              height: 18,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
        ],
      ),
    );
  }
}

class _Buy extends StatelessWidget {
  const _Buy({required this.controller});

  final AccountController controller;

  @override
  Widget build(BuildContext context) {
    final theme = DocScanTheme.of(context);
    final packages = controller.packages;

    // All rails off means the server has no payment section configured. That
    // is a server state and never a defect here, so it draws as nothing rather
    // than as an error.
    if (packages == null || !packages.canBuy) return const SizedBox.shrink();

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SizedBox(height: 26),
        Text(
          'Add credits',
          style: TextStyle(
            color: theme.textStrong,
            fontSize: 17,
            fontWeight: FontWeight.w600,
          ),
        ),
        const SizedBox(height: 4),
        Text(
          'Credits are shared across our apps. OpenDocScan spends none of '
          'them — everything it does runs on this phone.',
          style: TextStyle(color: theme.textMuted, fontSize: 13, height: 1.4),
        ),
        const SizedBox(height: 14),
        for (final package in packages.packages)
          Padding(
            padding: const EdgeInsets.only(bottom: 10),
            child: OutlinedButton(
              key: Key('buy-${package.id}'),
              onPressed: packages.stripe ? () => controller.buy(package) : null,
              child: Row(
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                children: [
                  Text('${package.credits} credits'),
                  Text(
                    package.price,
                    style: TextStyle(
                      color: theme.textMuted,
                      fontFeatures: const [FontFeature.tabularFigures()],
                    ),
                  ),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

class _Ledger extends StatelessWidget {
  const _Ledger({required this.controller});

  final AccountController controller;

  @override
  Widget build(BuildContext context) {
    final theme = DocScanTheme.of(context);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SizedBox(height: 26),
        Text(
          'Where credits went',
          style: TextStyle(
            color: theme.textStrong,
            fontSize: 17,
            fontWeight: FontWeight.w600,
          ),
        ),
        const SizedBox(height: 10),
        if (controller.entries.isEmpty)
          Text(
            'Nothing yet.',
            key: const Key('ledger-empty'),
            style: TextStyle(color: theme.textMuted, fontSize: 14),
          )
        else
          // Not scoped to this app. The account is shared, so filtering to
          // OpenDocScan would leave someone looking at a balance that dropped
          // for reasons this screen refused to name.
          for (final entry in controller.entries)
            _Entry(entry: entry, theme: theme),
        if (!controller.historyComplete && controller.entries.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: TextButton(
              key: const Key('show-earlier'),
              onPressed: controller.busy ? null : controller.loadMore,
              child: const Text('Show earlier'),
            ),
          ),
      ],
    );
  }
}

class _Entry extends StatelessWidget {
  const _Entry({required this.entry, required this.theme});

  final CreditEntry entry;
  final DocScanTheme theme;

  @override
  Widget build(BuildContext context) {
    final credit = entry.amount >= 0;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 11),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  entry.label,
                  style: TextStyle(color: theme.textStrong, fontSize: 14),
                ),
                const SizedBox(height: 3),
                Text(
                  _shortDate(entry.createdAt),
                  style: TextStyle(color: theme.textMuted, fontSize: 12),
                ),
              ],
            ),
          ),
          Text(
            '${credit ? '+' : ''}${entry.amount}',
            style: TextStyle(
              color: credit ? theme.brand : theme.textMuted,
              fontSize: 14,
              fontWeight: FontWeight.w600,
              fontFeatures: const [FontFeature.tabularFigures()],
            ),
          ),
        ],
      ),
    );
  }
}

String _shortDate(DateTime when) {
  final local = when.toLocal();
  const months = [
    'Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', //
    'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec',
  ];
  return '${local.day} ${months[local.month - 1]} ${local.year}';
}

class _SignOut extends StatelessWidget {
  const _SignOut({required this.controller});

  final AccountController controller;

  @override
  Widget build(BuildContext context) {
    return OutlinedButton(
      key: const Key('sign-out'),
      onPressed: controller.signOut,
      child: const Text('Sign out'),
    );
  }
}

class _Error extends StatelessWidget {
  const _Error({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    final theme = DocScanTheme.of(context);
    return Container(
      key: const Key('account-error'),
      margin: const EdgeInsets.only(top: 16),
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        border: Border.all(color: theme.danger.withValues(alpha: 0.5)),
        borderRadius: BorderRadius.circular(12),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(Icons.error_outline, size: 18, color: theme.danger),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              message,
              style: TextStyle(color: theme.textMuted, fontSize: 13),
            ),
          ),
        ],
      ),
    );
  }
}
