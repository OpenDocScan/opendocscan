import 'dart:typed_data';

import 'package:docscan/capture/capture_controller.dart';
import 'package:docscan/import/file_picker_platform.dart';
import 'package:docscan/permissions/permission_gate.dart';
import 'package:flutter/widgets.dart';
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
