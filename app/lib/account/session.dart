import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

/// A signed-in session: the pair of tokens the account server issues.
///
/// The access token is short-lived and sent as a bearer on every authenticated
/// call. The refresh token is the long-lived one — it is the credential worth
/// protecting, and it is the reason [SecureSessionStore] exists rather than a
/// plain preferences file.
@immutable
class Session {
  const Session({required this.accessToken, required this.refreshToken});

  final String accessToken;
  final String refreshToken;

  static Session? fromJson(Map<String, dynamic> json) {
    final access = json['access_token'];
    final refresh = json['refresh_token'];
    if (access is! String || refresh is! String) return null;
    if (access.isEmpty || refresh.isEmpty) return null;
    return Session(accessToken: access, refreshToken: refresh);
  }

  Map<String, dynamic> toJson() => {
    'access_token': accessToken,
    'refresh_token': refreshToken,
  };

  @override
  bool operator ==(Object other) =>
      other is Session &&
      other.accessToken == accessToken &&
      other.refreshToken == refreshToken;

  @override
  int get hashCode => Object.hash(accessToken, refreshToken);

  /// Never the tokens themselves. A session in a log line or an error report is
  /// a session someone else can use.
  @override
  String toString() => 'Session(<redacted>)';
}

/// Where the session lives between launches.
abstract interface class SessionStore {
  Future<Session?> read();
  Future<void> write(Session? session);
}

/// The real one: the iOS keychain, and on Android an AES-GCM store whose key is
/// wrapped in the hardware keystore.
///
/// The obvious alternative — `shared_preferences` — is a plaintext XML file in
/// the app's data directory, readable on any rooted or jailbroken device and by
/// anything holding a backup of it. A refresh token there is a durable
/// credential sitting in the clear.
///
/// `first_unlock` rather than the stricter `unlocked`: the session is read
/// during launch, and a phone that has been unlocked once since boot is the
/// condition that actually holds then. `unlocked` reads back nothing when the
/// app is started from a locked screen, which surfaces as being mysteriously
/// signed out.
class SecureSessionStore implements SessionStore {
  const SecureSessionStore([
    this.storage = const FlutterSecureStorage(
      iOptions: IOSOptions(accessibility: KeychainAccessibility.first_unlock),
    ),
  ]);

  final FlutterSecureStorage storage;

  static const _key = 'account.session';

  @override
  Future<Session?> read() async {
    final raw = await storage.read(key: _key);
    if (raw == null) return null;
    try {
      final decoded = jsonDecode(raw);
      if (decoded is! Map<String, dynamic>) return null;
      return Session.fromJson(decoded);
    } on FormatException {
      // A store that cannot be parsed is a store that cannot sign anyone in.
      // Treat it as signed out rather than crashing on launch.
      return null;
    }
  }

  @override
  Future<void> write(Session? session) async {
    if (session == null) {
      await storage.delete(key: _key);
    } else {
      await storage.write(key: _key, value: jsonEncode(session.toJson()));
    }
  }
}

/// For tests, and for the one host platform this app is exercised on where no
/// keychain exists.
class MemorySessionStore implements SessionStore {
  MemorySessionStore([this._session]);

  Session? _session;
  int writes = 0;

  @override
  Future<Session?> read() async => _session;

  @override
  Future<void> write(Session? session) async {
    writes++;
    _session = session;
  }
}
