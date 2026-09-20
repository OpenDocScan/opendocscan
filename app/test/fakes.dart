import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:docscan/account/account_api.dart';
import 'package:docscan/account/account_controller.dart';
import 'package:docscan/account/session.dart';
import 'package:docscan/account/sign_in.dart';
import 'package:docscan/capture/capture_controller.dart';
import 'package:docscan/import/file_picker_platform.dart';
import 'package:docscan/permissions/permission_gate.dart';
import 'package:flutter/widgets.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:permission_handler/permission_handler.dart';

/// Stand-ins for the three things only a phone can provide.
///
/// Each records what it was asked, because several of the tests below are
/// about *not* doing something — not taking two photographs on a double tap,
/// not treating a cancelled pick as an error.

class FakePicker implements FilePickerPlatform {
  FakePicker(this.images);

  List<Uint8List> images;
  int calls = 0;

  @override
  Future<List<Uint8List>> pickImages() async {
    calls++;
    return images;
  }
}

class FakePermissions implements PermissionGate {
  FakePermissions({this.granted = true, this.permanentlyDenied = false});

  bool granted;
  bool permanentlyDenied;
  int requests = 0;

  @override
  Future<bool> request(Permission permission) async {
    requests++;
    return granted;
  }

  @override
  Future<bool> isPermanentlyDenied(Permission permission) async =>
      permanentlyDenied;
}

class FakeCamera implements CaptureController {
  FakeCamera({this.bytes, this.failOnInitialise, this.captureDelay});

  Uint8List? bytes;
  String? failOnInitialise;
  Duration? captureDelay;

  int captures = 0;
  bool disposed = false;

  @override
  Future<void> initialise() async {
    if (failOnInitialise != null) throw StateError(failOnInitialise!);
  }

  @override
  Widget? preview() => const SizedBox(key: Key('fake-preview'));

  @override
  Future<Uint8List> capture() async {
    captures++;
    if (captureDelay != null) await Future<void>.delayed(captureDelay!);
    return bytes ?? Uint8List.fromList([1, 2, 3]);
  }

  @override
  Future<void> dispose() async {
    disposed = true;
  }
}

/// A one-pixel PNG. Enough for `Image.memory` to render without a decode
/// error, which is what the result card does with whatever it is handed.
final Uint8List onePixelPng = Uint8List.fromList([
  0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, //
  0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
  0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
  0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
  0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41,
  0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
  0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
  0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
  0x42, 0x60, 0x82,
]);

// --------------------------------------------------------------- the account

/// A browser that opens nothing and remembers what it was asked to open.
///
/// Which URL sign-in opens is most of what can go wrong with it — a wrong
/// `return_to`, a fragment where the server forbids one — so it is recorded
/// rather than fired into the void.
class FakeBrowser implements Browser {
  FakeBrowser({this.succeeds = true});

  bool succeeds;
  final List<Uri> opened = [];

  @override
  Future<bool> open(Uri url) async {
    opened.add(url);
    return succeeds;
  }
}

/// Links from the operating system, under the test's control.
class FakeDeepLinks implements DeepLinks {
  FakeDeepLinks({this.cold});

  /// The link that launched the app, if it was launched by one.
  Uri? cold;

  final StreamController<Uri> _warm = StreamController<Uri>.broadcast();

  @override
  Future<Uri?> initial() async => cold;

  @override
  Stream<Uri> get stream => _warm.stream;

  /// A link arriving while the app is already running.
  void arrive(Uri uri) => _warm.add(uri);

  Future<void> close() => _warm.close();
}

/// An [AccountApi] wired to a scripted server.
///
/// [respond] is given the method and path — `'POST /v1/auth/oidc/exchange'` —
/// and returns the status and body. Requests are recorded, because several of
/// the tests below are about a request *not* being made twice.
class FakeServer {
  FakeServer(this.respond);

  final (int, Object?) Function(String route) respond;
  final List<String> routes = [];

  http.Client get client => MockClient((request) async {
    final route = '${request.method} ${request.url.path}';
    routes.add(route);
    final (status, body) = respond(route);
    return http.Response(
      body == null ? '' : jsonEncode(body),
      status,
      headers: {'content-type': 'application/json'},
    );
  });

  AccountApi api({Session? signedInAs}) => AccountApi(
    client: client,
    store: MemorySessionStore(signedInAs),
  );
}

/// A session with tokens that are obviously not real ones.
const fakeSession = Session(accessToken: 'access-1', refreshToken: 'refresh-1');

/// A client where every request fails at the transport, the way a phone with no
/// signal fails.
class ThrowingClient extends http.BaseClient {
  @override
  Future<http.StreamedResponse> send(http.BaseRequest request) =>
      Future.error(const SocketException('no route to host'));
}

/// Sugar for the handful of tests that need an unauthenticated call only.
AccountApi anonymousApi(FakeServer server) =>
    AccountApi(client: server.client, store: MemorySessionStore());

/// An account controller with nowhere to call and nothing stored.
///
/// The scanner's own tests are not about the account, but the way in sits in
/// its app bar, so they need one. This is signed out and offline.
///
/// Started, because the real app starts it at launch. An unstarted controller
/// sits in [AccountStage.loading] forever, which draws a spinner that never
/// stops — `pumpAndSettle` hangs on it, and the first version of this helper
/// did exactly that.
AccountController offlineAccount() {
  final controller = AccountController(
    api: AccountApi(client: ThrowingClient(), store: MemorySessionStore()),
    browser: FakeBrowser(),
  );
  unawaited(controller.start());
  return controller;
}
