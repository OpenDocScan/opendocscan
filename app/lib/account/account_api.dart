import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:http/http.dart' as http;

import 'openapps.dart';
import 'session.dart';

/// Anything the account server, or the network under it, can go wrong with.
///
/// [code] is the machine-readable half and is what the UI branches on. Two
/// values matter more than the rest:
///
/// * `network` — the request never got an answer. On the web this is what CORS
///   looks like from the outside; here it is genuinely a dead connection, an
///   aeroplane, or a phone that has just walked into a lift.
/// * `unauthorized` — the session is gone or was never there. The caller signs
///   the user out rather than retrying.
@immutable
class AccountError implements Exception {
  const AccountError(this.code, this.message);

  final String code;
  final String message;

  bool get isUnauthorized => code == 'unauthorized';

  @override
  String toString() => 'AccountError($code): $message';
}

/// A credit ledger entry, as `<openapps-history>` renders it on the web.
@immutable
class CreditEntry {
  const CreditEntry({
    required this.kind,
    required this.amount,
    required this.createdAt,
    this.appName,
    this.appId,
    this.refId,
  });

  /// `debit`, `topup`, `referral_bonus`, `adjustment` or `refund`.
  final String kind;

  /// Signed: negative is spending.
  final int amount;
  final DateTime createdAt;
  final String? appName;
  final String? appId;
  final String? refId;

  static CreditEntry fromJson(Map<String, dynamic> json) => CreditEntry(
    kind: json['kind'] as String? ?? 'debit',
    amount: (json['amount'] as num?)?.toInt() ?? 0,
    createdAt: DateTime.fromMillisecondsSinceEpoch(
      ((json['created_at'] as num?)?.toInt() ?? 0) * 1000,
    ),
    appName: json['app_name'] as String?,
    appId: json['app_id'] as String?,
    refId: json['ref_id'] as String?,
  );

  /// What the credits went on, phrased for a reader rather than a developer.
  /// The same wording the web element uses, so a person who checks their
  /// history in both places sees one account rather than two.
  String get label => switch (kind) {
    'topup' => 'Credits purchased',
    'referral_bonus' => 'Referral bonus',
    'refund' => amount < 0 ? 'Payment reversed' : 'Refund',
    'adjustment' => refId == null ? 'Adjustment' : 'Adjustment — $refId',
    _ => _spentLabel,
  };

  String get _spentLabel {
    final app = appName ?? appId;
    if (app != null && refId != null) return '$app · $refId';
    return app ?? refId ?? 'Spent';
  }
}

/// One thing that can be bought, straight from the server's own list.
@immutable
class CreditPackage {
  const CreditPackage({
    required this.id,
    required this.credits,
    required this.usdCents,
  });

  final String id;
  final int credits;
  final int usdCents;

  static CreditPackage fromJson(Map<String, dynamic> json) => CreditPackage(
    id: json['id'] as String? ?? '',
    credits: (json['credits'] as num?)?.toInt() ?? 0,
    usdCents: (json['usd_price'] as num?)?.toInt() ?? 0,
  );

  String get price => '\$${(usdCents / 100).toStringAsFixed(2)}';
}

/// What can be bought and how. All three rails may be off, in which case the
/// server has no payment section configured and there is nothing to show —
/// that is a server state, never a bug in this app.
@immutable
class Packages {
  const Packages({required this.packages, required this.rails});

  final List<CreditPackage> packages;
  final Set<String> rails;

  bool get canBuy => packages.isNotEmpty && rails.isNotEmpty;
  bool get stripe => rails.contains('stripe');
}

/// One page of the ledger.
@immutable
class History {
  const History({required this.entries, this.nextCursor});

  final List<CreditEntry> entries;
  final String? nextCursor;

  bool get complete => nextCursor == null;
}

/// Everything this app asks the account server for.
///
/// Every request in here goes through [_send], and [_send] refuses any URL that
/// is not on [isAccountHost]. That check is the whole reason this class is a
/// single narrow door rather than a handful of call sites with a `http.get` in
/// each: the Android release build now carries `INTERNET`, and this is what
/// replaces the guarantee that permission used to give for free.
class AccountApi {
  AccountApi({http.Client? client, required this.store})
    : _client = client ?? http.Client();

  final http.Client _client;
  final SessionStore store;

  /// The session as this process currently understands it. Read through
  /// [load] before the first authenticated call — a call made against an
  /// unread store looks exactly like being signed out.
  Session? session;

  Future<Session?>? _refreshing;

  Future<Session?> load() async => session = await store.read();

  bool get isSignedIn => session != null;

  Future<void> _remember(Session? next) async {
    session = next;
    await store.write(next);
  }

  // ---------------------------------------------------------------- requests

  Future<Map<String, dynamic>> _send(
    String method,
    String path, {
    Map<String, dynamic>? body,
    Map<String, String>? query,
    bool authenticated = false,
    bool allowRefresh = true,
  }) async {
    final url = Uri.parse(
      '$kAuthBase$path',
    ).replace(queryParameters: query?.isEmpty ?? true ? null : query);

    // The door. Not a formality: a mistyped constant or a future contributor's
    // convenience endpoint would otherwise be a document scanner quietly
    // talking to a host nobody audited.
    if (!isAccountHost(url)) {
      throw AccountError(
        'blocked',
        'refusing to contact ${url.host} — this app talks to one host',
      );
    }

    final headers = <String, String>{'accept': 'application/json'};
    if (body != null) headers['content-type'] = 'application/json';
    if (authenticated) {
      final token = session?.accessToken;
      if (token == null) {
        throw const AccountError('unauthorized', 'not signed in');
      }
      headers['authorization'] = 'Bearer $token';
    }

    http.Response response;
    try {
      final request = http.Request(method, url)..headers.addAll(headers);
      if (body != null) request.body = jsonEncode(body);
      response = await http.Response.fromStream(await _client.send(request));
    } on Object catch (error) {
      // Every transport failure lands here — no DNS, no route, a dropped TLS
      // handshake. One message, because to the person holding the phone they
      // are one situation.
      throw AccountError('network', 'Could not reach the server ($error)');
    }

    // A 401 on a call we thought was authenticated means the access token
    // aged out. Spend the refresh token once and replay; if that fails the
    // session is genuinely over.
    if (response.statusCode == 401 && authenticated && allowRefresh) {
      final refreshed = await _refresh();
      if (refreshed == null) {
        throw const AccountError('unauthorized', 'the session has expired');
      }
      return _send(
        method,
        path,
        body: body,
        query: query,
        authenticated: true,
        allowRefresh: false,
      );
    }

    final decoded = response.body.isEmpty ? null : _decode(response.body);

    if (response.statusCode >= 400) {
      // A 401 means two entirely different things depending on what carried the
      // credential. On an authenticated call the bearer token is dead and the
      // session is over. On an unauthenticated one — the exchange, the refresh —
      // it is the credential *in the body* that was rejected, and there is no
      // session to end. Treating them alike is not academic: a rejected sign-in
      // code showed a signed-out person the words "the session has expired",
      // which is both wrong and unactionable, and it took an emulator to see it.
      if (response.statusCode == 401 && authenticated) {
        await _remember(null);
        throw const AccountError('unauthorized', 'the session has expired');
      }
      final error = decoded?['error'];
      throw AccountError(
        error is Map && error['code'] is String
            ? error['code'] as String
            : 'http_${response.statusCode}',
        error is Map && error['message'] is String
            ? error['message'] as String
            : 'The server answered ${response.statusCode}.',
      );
    }

    return decoded ?? const {};
  }

  Map<String, dynamic>? _decode(String body) {
    try {
      final value = jsonDecode(body);
      return value is Map<String, dynamic> ? value : null;
    } on FormatException {
      return null;
    }
  }

  /// Single-flight, because several widgets can discover an expired token in
  /// the same frame. Two refreshes racing means the loser presents an already
  /// rotated refresh token and gets the user signed out for no reason.
  Future<Session?> _refresh() {
    final inFlight = _refreshing;
    if (inFlight != null) return inFlight;

    final current = session;
    if (current == null) return Future.value(null);

    return _refreshing = () async {
      try {
        final json = await _send(
          'POST',
          '/v1/auth/refresh',
          body: {'refresh_token': current.refreshToken},
        );
        final next = Session.fromJson(json);
        await _remember(next);
        return next;
      } on AccountError {
        await _remember(null);
        return null;
      } finally {
        _refreshing = null;
      }
    }();
  }

  // ------------------------------------------------------------------ public

  /// Where the browser is sent to begin a Google sign-in.
  Uri signInUrl({String? referral}) => Uri.parse(
    '$kAuthBase/v1/auth/oidc/google/start',
  ).replace(
    queryParameters: {
      'return_to': kSignInReturnTo,
      'ref': ?referral,
    },
  );

  /// Spend the one-time code the browser handed back. It is one-time in the
  /// strict sense: a replayed code is refused, which is why the trampoline page
  /// forwards it rather than exchanging it itself.
  Future<Session> exchange(String code) async {
    final Map<String, dynamic> json;
    try {
      json = await _send('POST', '/v1/auth/oidc/exchange', body: {'code': code});
    } on AccountError catch (failure) {
      // The server says "missing or invalid credentials", which is accurate and
      // says nothing a person can act on. A code is one-time and short-lived, so
      // the two things that actually happened are worth naming.
      if (failure.isUnauthorized) {
        throw const AccountError(
          'sign_in_failed',
          'That sign-in could not be completed — the link may already have been '
              'used, or taken too long. Try signing in again.',
        );
      }
      rethrow;
    }
    final next = Session.fromJson(json);
    if (next == null) {
      throw const AccountError('bad_response', 'the server sent no session');
    }
    await _remember(next);
    return next;
  }

  Future<int> balance() async {
    final json = await _send('GET', '/v1/credits/balance', authenticated: true);
    return (json['balance'] as num?)?.toInt() ?? 0;
  }

  Future<History> history({String? cursor, int limit = 20}) async {
    final json = await _send(
      'GET',
      '/v1/credits/history',
      authenticated: true,
      query: {'limit': '$limit', 'cursor': ?cursor},
    );
    final entries = (json['entries'] as List? ?? const [])
        .whereType<Map<String, dynamic>>()
        .map(CreditEntry.fromJson)
        .toList();
    return History(entries: entries, nextCursor: json['next_cursor'] as String?);
  }

  Future<Packages> packages() async {
    final json = await _send('GET', '/v1/payments/packages');
    final rails = (json['rails'] as Map?) ?? const {};
    return Packages(
      packages: (json['packages'] as List? ?? const [])
          .whereType<Map<String, dynamic>>()
          .map(CreditPackage.fromJson)
          .toList(),
      rails: {
        for (final entry in rails.entries)
          if (entry.value == true) '${entry.key}',
      },
    );
  }

  /// A Stripe checkout page to open in the browser.
  ///
  /// Sent with no `return_to`, deliberately. Stripe cannot redirect back into
  /// an app, so the purchase ends on the server's own confirmation page and the
  /// balance is refreshed when the app is next resumed.
  Future<Uri> checkout(String packageId) async {
    final json = await _send(
      'POST',
      '/v1/payments/stripe/checkout',
      authenticated: true,
      body: {'package_id': packageId},
    );
    final url = json['url'];
    if (url is! String) {
      throw const AccountError('bad_response', 'the server sent no checkout link');
    }
    return Uri.parse(url);
  }

  /// Ends the session on the server, then locally.
  ///
  /// The local half runs whatever the server says. A user who taps sign out on
  /// a train has to end up signed out on the device, or the control is a lie.
  Future<void> signOut() async {
    try {
      await _send('POST', '/v1/auth/logout', authenticated: true);
    } on AccountError {
      // Deliberately swallowed; see above.
    } finally {
      await _remember(null);
    }
  }
}
