import 'dart:typed_data';

import 'package:docscan/src/rust/api/scanner.dart' as rust;

/// What came back from the core for one photograph.
///
/// The three cases are separate types rather than a nullable field, because
/// the UI has to render each of them differently and a nullable field lets a
/// screen forget one. `failed` in particular is not hypothetical — an iOS HEIC
/// picked out of the Photos library lands there on every device.
sealed class ScanResult {
  const ScanResult();
}

class ScanPending extends ScanResult {
  const ScanPending();
}

class ScanDecoded extends ScanResult {
  const ScanDecoded({required this.bytes, required this.width, required this.height});

  final Uint8List bytes;
  final int width;
  final int height;

  int get pixels => width * height;
}

class ScanFailed extends ScanResult {
  const ScanFailed(this.message);

  final String message;
}

/// Send a photograph through the Rust core and report what it made of it.
///
/// This is the whole M1 acceptance path: bytes in from the camera or the file
/// picker, across the bridge, decoded by the same `docscan-core` the web build
/// uses, and the dimensions back on screen. Nothing here inspects the image —
/// that is the core's job, and keeping it that way is what stops image
/// processing leaking into Dart.
Future<ScanResult> decodeThroughCore(Uint8List bytes) async {
  try {
    final info = await rust.decodeDimensions(bytes: bytes);
    return ScanDecoded(bytes: bytes, width: info.width, height: info.height);
  } catch (e) {
    // The Rust side returns Err rather than panicking, so this is an ordinary
    // Dart exception and the app stays up. See the note on `decode_dimensions`.
    return ScanFailed(_readable(e));
  }
}

/// `flutter_rust_bridge` wraps the error string; the raw `toString()` carries
/// its own type name, which is noise to someone holding a phone.
String _readable(Object error) {
  final text = error.toString();
  final match = RegExp(r'[Ee]rror\s*[:(]\s*(.+?)\)?$').firstMatch(text);
  final message = match?.group(1)?.trim() ?? text;
  return message.isEmpty ? 'This file could not be read as an image.' : message;
}
