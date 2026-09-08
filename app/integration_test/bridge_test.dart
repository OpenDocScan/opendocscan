import 'dart:convert';
import 'dart:typed_data';

import 'package:docscan/scan_result.dart';
import 'package:docscan/src/rust/api/scanner.dart';
import 'package:docscan/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

/// The M1 acceptance test, run on a real device or emulator.
///
/// Everything in `test/` fakes the bridge, because `flutter test` runs on the
/// Dart VM where the Rust library for a phone cannot be loaded. So the host
/// suite proves the *screens* are right and proves nothing at all about the
/// bridge. This file is the other half: it loads the real `.so`, calls the
/// real `docscan-core`, and is the only thing that would catch a broken
/// cross-compile, a missing ABI, or a codegen that drifted from the Rust.
///
///   `flutter test integration_test/bridge_test.dart -d <device>`
void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async => RustLib.init());

  testWidgets('the library loads and answers', (tester) async {
    // Fails differently from every later problem: a missing library rather
    // than a wrong answer. Check it first so the others are interpretable.
    expect(await ping(), 'docscan-ffi-ok');
  });

  testWidgets('a PNG crosses the bridge and comes back measured',
      (tester) async {
    final info = await decodeDimensions(bytes: _png(37, 19));

    expect(info.width, 37);
    expect(info.height, 19);
  });

  testWidgets('a JPEG crosses the bridge — which is what a camera produces',
      (tester) async {
    // The workspace pins `image` with default-features off. If the `jpeg`
    // feature is ever trimmed to what the browser build needs, the camera
    // stops working on both phones and only this fails.
    final info = await decodeDimensions(bytes: _jpeg());

    expect(info.width, 64);
    expect(info.height, 48);
  });

  testWidgets('a megapixel photograph survives the crossing intact',
      (tester) async {
    // The real payload is a few megabytes, not a few hundred bytes, and the
    // bridge copies it. A size that works at 100 bytes and not at 4 MB is the
    // kind of thing that only shows up on a phone.
    final info = await decodeDimensions(bytes: _png(1920, 1080));

    expect(info.width, 1920);
    expect(info.height, 1080);
  });

  testWidgets('bad bytes raise a catchable error rather than killing the app',
      (tester) async {
    // The case the plan deferred with a `todo`. A panic here would take the
    // whole process down; this asserts it does not.
    await expectLater(
      decodeDimensions(bytes: Uint8List.fromList([1, 2, 3, 4])),
      throwsA(anything),
    );

    // And the app is still alive to answer afterwards.
    expect(await ping(), 'docscan-ffi-ok');
  });

  testWidgets('the app layer turns that error into a result, not a crash',
      (tester) async {
    final result = await decodeThroughCore(Uint8List.fromList([9, 9, 9]));

    expect(result, isA<ScanFailed>());
    expect((result as ScanFailed).message, isNotEmpty);
  });
}

/// A minimal uncompressed-ish PNG built by hand, so the test depends on no
/// image package on the Dart side — the point is what *Rust* makes of it.
Uint8List _png(int width, int height) {
  final ihdr = BytesBuilder()
    ..add(_be32(width))
    ..add(_be32(height))
    ..add([8, 2, 0, 0, 0]); // 8-bit, truecolour RGB

  // One filter byte per row, then 3 bytes a pixel, all zero: a black image.
  final raw = BytesBuilder();
  for (var y = 0; y < height; y++) {
    raw.addByte(0);
    raw.add(Uint8List(width * 3));
  }
  final idat = _zlibStore(raw.takeBytes());

  return Uint8List.fromList([
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
    ..._chunk('IHDR', ihdr.takeBytes()),
    ..._chunk('IDAT', idat),
    ..._chunk('IEND', Uint8List(0)),
  ]);
}

/// A real 64x48 baseline JPEG, encoded once and embedded.
///
/// The first version of this test hand-assembled a JPEG byte by byte and it
/// was not a valid one — a DC Huffman table, no AC table, and a single
/// entropy-coded byte. The Rust decoder rejected it, correctly, and the test
/// failed for a reason that had nothing to do with the bridge. A format with
/// entropy coding is not one to write by hand just to avoid a constant.
Uint8List _jpeg() => base64Decode(
    '/9j/4AAQSkZJRgABAQAASABIAAD/4QBMRXhpZgAATU0AKgAAAAgAAYdpAAQAAAABAAAAGgAAAAAA'
    'A6ABAAMAAAABAAEAAKACAAQAAAABAAAAQKADAAQAAAABAAAAMAAAAAD/7QA4UGhvdG9zaG9wIDMu'
    'MAA4QklNBAQAAAAAAAA4QklNBCUAAAAAABDUHYzZjwCyBOmACZjs+EJ+/8AAEQgAMABAAwEiAAIR'
    'AQMRAf/EAB8AAAEFAQEBAQEBAAAAAAAAAAABAgMEBQYHCAkKC//EALUQAAIBAwMCBAMFBQQEAAAB'
    'fQECAwAEEQUSITFBBhNRYQcicRQygZGhCCNCscEVUtHwJDNicoIJChYXGBkaJSYnKCkqNDU2Nzg5'
    'OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6g4SFhoeIiYqSk5SVlpeYmZqio6Slpqeo'
    'qaqys7S1tre4ubrCw8TFxsfIycrS09TV1tfY2drh4uPk5ebn6Onq8fLz9PX29/j5+v/EAB8BAAMB'
    'AQEBAQEBAQEAAAAAAAABAgMEBQYHCAkKC//EALURAAIBAgQEAwQHBQQEAAECdwABAgMRBAUhMQYS'
    'QVEHYXETIjKBCBRCkaGxwQkjM1LwFWJy0QoWJDThJfEXGBkaJicoKSo1Njc4OTpDREVGR0hJSlNU'
    'VVZXWFlaY2RlZmdoaWpzdHV2d3h5eoKDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5'
    'usLDxMXGx8jJytLT1NXW19jZ2uLj5OXm5+jp6vLz9PX29/j5+v/bAEMABAQEBAQEBgQEBgkGBgYJ'
    'DAkJCQkMDwwMDAwMDxIPDw8PDw8SEhISEhISEhUVFRUVFRkZGRkZHBwcHBwcHBwcHP/bAEMBBAUF'
    'BwcHDAcHDB0UEBQdHR0dHR0dHR0dHR0dHR0dHR0dHR0dHR0dHR0dHR0dHR0dHR0dHR0dHR0dHR0d'
    'HR0dHf/dAAQABP/aAAwDAQACEQMRAD8A+WfBH/LP8K+wPBH/ACz/AAr4/wDBH/LP8K+wPBH/ACz/'
    'AArYzNP9pD/kkMH/AGFLb/0CSvmzwR/yz/CvpP8AaQ/5JDB/2FLb/wBAkr5s8Ef8s/wrKHxT9f0R'
    '6GK/hUP8L/8AS5n2B4I/5Z/hXzx+01/yVfTf+wNb/wDo+evofwR/yz/Cvnj9pr/kq+m/9ga3/wDR'
    '89cmK/i0P8T/APSJnFHZk/gj/ln+FfYHgj/ln+FfH/gj/ln+FfYHgj/ln+FegQf/0PlnwR/yz/Cv'
    'sDwR/wAs/wAK+P8AwR/yz/CvsDwR/wAs/wAK2MzT/aQ/5JDB/wBhS2/9Akr5s8Ef8s/wr6T/AGkP'
    '+SQwf9hS2/8AQJK+bPBH/LP8Kyh8U/X9Eehiv4VD/C//AEuZ9geCP+Wf4V88ftNf8lX03/sDW/8A'
    '6Pnr6H8Ef8s/wr54/aa/5Kvpv/YGt/8A0fPXJiv4tD/E/wD0iZxR2ZP4I/5Z/hX2B4I/5Z/hXx/4'
    'I/5Z/hX2B4I/5Z/hXoEH/9H5Z8Ef8s/wr7A8Ef8ALP8ACvj/AMEf8s/wr7A8Ef8ALP8ACtjM0/2k'
    'P+SQwf8AYUtv/QJK+bPBH/LP8K+k/wBpD/kkMH/YUtv/AECSvmzwR/yz/CsofFP1/RHoYr+FQ/wv'
    '/wBLmfYHgj/ln+FfPH7TX/JV9N/7A1v/AOj56+h/BH/LP8K+eP2mv+Sr6b/2Brf/ANHz1yYr+LQ/'
    'xP8A9ImcUdmT+CP+Wf4V9geCP+Wf4V8f+CP+Wf4V9geCP+Wf4V6BB//Z',
    );

List<int> _chunk(String type, Uint8List body) {
  final data = <int>[...type.codeUnits, ...body];
  return [..._be32(body.length), ...data, ..._be32(_crc32(data))];
}

List<int> _be32(int v) => [(v >> 24) & 0xFF, (v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF];

/// zlib with stored (uncompressed) deflate blocks — valid, and avoids pulling
/// a compressor into a test whose subject is the Rust side.
Uint8List _zlibStore(Uint8List data) {
  final out = BytesBuilder()..add([0x78, 0x01]);
  var offset = 0;
  while (offset < data.length) {
    final len = (data.length - offset).clamp(0, 65535);
    final last = offset + len >= data.length ? 1 : 0;
    out.add([last, len & 0xFF, (len >> 8) & 0xFF, (~len) & 0xFF, ((~len) >> 8) & 0xFF]);
    out.add(data.sublist(offset, offset + len));
    offset += len;
  }
  out.add(_be32(_adler32(data)));
  return out.takeBytes();
}

int _adler32(Uint8List data) {
  var a = 1, b = 0;
  for (final byte in data) {
    a = (a + byte) % 65521;
    b = (b + a) % 65521;
  }
  return (b << 16) | a;
}

final List<int> _crcTable = List<int>.generate(256, (n) {
  var c = n;
  for (var k = 0; k < 8; k++) {
    c = (c & 1) != 0 ? 0xEDB88320 ^ (c >> 1) : c >> 1;
  }
  return c;
});

int _crc32(List<int> data) {
  var c = 0xFFFFFFFF;
  for (final byte in data) {
    c = _crcTable[(c ^ byte) & 0xFF] ^ (c >> 8);
  }
  return (c ^ 0xFFFFFFFF) & 0xFFFFFFFF;
}
