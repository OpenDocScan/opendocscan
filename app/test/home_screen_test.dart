import 'dart:async';
import 'dart:typed_data';

import 'package:docscan/home_screen.dart';
import 'package:docscan/scan_result.dart';
import 'package:docscan/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'fakes.dart';

void main() {
  Widget wrap({
    required FakePicker picker,
    FakePermissions? permissions,
    FakeCamera? camera,
    Decoder? decode,
  }) {
    return MaterialApp(
      theme: buildTheme(Brightness.dark),
      home: HomeScreen(
        picker: picker,
        permissions: permissions ?? FakePermissions(),
        captureControllerFactory: () => camera ?? FakeCamera(),
        decode: decode ??
            (bytes) async =>
                ScanDecoded(bytes: bytes, width: 1275, height: 1650),
      ),
    );
  }

  testWidgets('starts empty, and says where scans live', (tester) async {
    await tester.pumpWidget(wrap(picker: FakePicker([])));

    expect(find.text('Nothing scanned yet'), findsOneWidget);
    expect(find.textContaining('nothing is uploaded'), findsOneWidget);
  });

  testWidgets('an imported photograph shows the size the core reported',
      (tester) async {
    await tester.pumpWidget(wrap(picker: FakePicker([onePixelPng])));

    await tester.tap(find.byKey(const Key('import')));
    await tester.pumpAndSettle();

    // The assertion that matters: the number on screen came back from the
    // decoder, not from anything Dart measured. 1275x1650 is a 150dpi A4 page
    // and is nothing the one-pixel PNG could have produced by itself.
    expect(find.byKey(const Key('dimensions')), findsOneWidget);
    expect(find.text('1275 × 1650'), findsOneWidget);
  });

  testWidgets('every picked image gets its own result, in the order picked',
      (tester) async {
    // Distinct sizes per image, because a ListView does not build its
    // off-screen children — counting the cards on screen would count the
    // viewport, not the results. Scrolling to the last one is what proves all
    // three exist.
    var call = 0;
    await tester.pumpWidget(
      wrap(
        picker: FakePicker([onePixelPng, onePixelPng, onePixelPng]),
        decode: (bytes) async {
          call++;
          final size = call * 100;
          return ScanDecoded(bytes: bytes, width: size, height: size);
        },
      ),
    );

    await tester.tap(find.byKey(const Key('import')));
    await tester.pumpAndSettle();

    // Within one import the pick order is preserved — page 1 above page 2 —
    // while a later import goes above an earlier one. That is what a document
    // wants, and it is the behaviour worth pinning: reversing the batch would
    // silently put a multi-page scan in backwards.
    expect(find.text('100 × 100'), findsOneWidget);

    await tester.scrollUntilVisible(find.text('300 × 300'), 200);
    expect(find.text('300 × 300'), findsOneWidget);
    expect(find.text('200 × 200'), findsOneWidget);
  });

  testWidgets('a cancelled pick changes nothing and is not an error',
      (tester) async {
    final picker = FakePicker([]);
    await tester.pumpWidget(wrap(picker: picker));

    await tester.tap(find.byKey(const Key('import')));
    await tester.pumpAndSettle();

    expect(picker.calls, 1);
    expect(find.text('Nothing scanned yet'), findsOneWidget);
    expect(find.byKey(const Key('failure-message')), findsNothing);
  });

  testWidgets('an undecodable file reports itself and leaves the app up',
      (tester) async {
    await tester.pumpWidget(
      wrap(
        picker: FakePicker([Uint8List.fromList([1, 2, 3])]),
        decode: (_) async => const ScanFailed('unsupported image format'),
      ),
    );

    await tester.tap(find.byKey(const Key('import')));
    await tester.pumpAndSettle();

    expect(find.text('Could not read this image'), findsOneWidget);
    expect(find.text('unsupported image format'), findsOneWidget);
    // Still usable afterwards — the failure is a card, not a dead end.
    expect(find.byKey(const Key('import')), findsOneWidget);
  });

  testWidgets('one bad file among good ones does not lose the good ones',
      (tester) async {
    var call = 0;
    await tester.pumpWidget(
      wrap(
        picker: FakePicker([onePixelPng, onePixelPng, onePixelPng]),
        decode: (bytes) async {
          call++;
          if (call == 2) return const ScanFailed('truncated');
          return ScanDecoded(bytes: bytes, width: 800, height: 600);
        },
      ),
    );

    await tester.tap(find.byKey(const Key('import')));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('dimensions')), findsNWidgets(2));
    expect(find.text('truncated'), findsOneWidget);
  });

  testWidgets('a photograph taken with the camera goes through the same path',
      (tester) async {
    final camera = FakeCamera(bytes: onePixelPng);
    await tester.pumpWidget(
      wrap(picker: FakePicker([]), camera: camera),
    );

    await tester.tap(find.byKey(const Key('scan')));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('shutter')));
    await tester.pumpAndSettle();

    expect(camera.captures, 1);
    expect(find.text('1275 × 1650'), findsOneWidget);
  });

  testWidgets('backing out of the camera adds nothing', (tester) async {
    await tester.pumpWidget(wrap(picker: FakePicker([])));

    await tester.tap(find.byKey(const Key('scan')));
    await tester.pumpAndSettle();

    await tester.pageBack();
    await tester.pumpAndSettle();

    expect(find.text('Nothing scanned yet'), findsOneWidget);
  });

  testWidgets('the buttons are disabled while the core is working',
      (tester) async {
    final gate = Completer<ScanResult>();
    await tester.pumpWidget(
      wrap(
        picker: FakePicker([onePixelPng]),
        decode: (_) => gate.future,
      ),
    );

    await tester.tap(find.byKey(const Key('import')));
    await tester.pump();

    expect(find.byKey(const Key('busy')), findsOneWidget);
    final scan = tester.widget<FilledButton>(find.byKey(const Key('scan')));
    expect(scan.onPressed, isNull, reason: 'a second scan must not queue up');

    gate.complete(ScanDecoded(bytes: onePixelPng, width: 1, height: 1));
    await tester.pumpAndSettle();
  });
}
