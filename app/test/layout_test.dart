import 'package:docscan/capture/capture_screen.dart';
import 'package:docscan/home_screen.dart';
import 'package:docscan/layout.dart';
import 'package:docscan/scan_result.dart';
import 'package:docscan/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'fakes.dart';

/// iPad and iPhone layouts, driven by resizing the window rather than by
/// guessing at a device.
///
/// Every case below is reachable on one device: an iPad in Split View hands the
/// app a window narrower than an iPhone's, and rotating it changes the shape
/// without changing anything else. That is why nothing here asks what device
/// it is running on, and why these tests set a size rather than a platform.
void main() {
  Widget wrap({required FakePicker picker, Decoder? decode}) => MaterialApp(
        theme: buildTheme(Brightness.dark),
        home: HomeScreen(
          picker: picker,
          permissions: FakePermissions(),
          captureControllerFactory: FakeCamera.new,
          decode: decode ??
              (bytes) async =>
                  ScanDecoded(bytes: bytes, width: 1275, height: 1650),
        ),
      );

  Future<void> sized(WidgetTester tester, Size size, Widget child) async {
    tester.view.physicalSize = size;
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(child);
  }

  group('Pane', () {
    test('follows the window, and the boundaries are the documented ones', () {
      expect(Pane.forWidth(390), Pane.compact); // iPhone
      expect(Pane.forWidth(320), Pane.compact); // iPad, Split View, narrow
      expect(Pane.forWidth(599), Pane.compact);
      expect(Pane.forWidth(600), Pane.medium);
      expect(Pane.forWidth(834), Pane.medium); // iPad portrait, half width
      expect(Pane.forWidth(999), Pane.medium);
      expect(Pane.forWidth(1000), Pane.expanded);
      expect(Pane.forWidth(1366), Pane.expanded); // 12.9" landscape
    });
  });

  testWidgets('on a phone the results are one column', (tester) async {
    await sized(
      tester,
      const Size(390, 844),
      wrap(picker: FakePicker([onePixelPng, onePixelPng])),
    );

    await tester.tap(find.byKey(const Key('import')));
    await tester.pumpAndSettle();

    final cards = tester.widgetList(find.byKey(const Key('dimensions')));
    expect(cards, hasLength(2));

    // Both cards start at the same x — one per row.
    final first = tester.getTopLeft(find.byKey(const Key('dimensions')).first);
    final last = tester.getTopLeft(find.byKey(const Key('dimensions')).last);
    expect(first.dx, last.dx);
    expect(first.dy, isNot(last.dy));
  });

  testWidgets('on an iPad the results sit side by side', (tester) async {
    // 12.9-inch landscape.
    await sized(
      tester,
      const Size(1366, 1024),
      wrap(picker: FakePicker([onePixelPng, onePixelPng])),
    );

    await tester.tap(find.byKey(const Key('import')));
    await tester.pumpAndSettle();

    final first = tester.getTopLeft(find.byKey(const Key('dimensions')).first);
    final last = tester.getTopLeft(find.byKey(const Key('dimensions')).last);
    expect(first.dy, last.dy, reason: 'same row');
    expect(first.dx, isNot(last.dx), reason: 'different columns');
  });

  testWidgets('the action buttons stop growing past a readable width',
      (tester) async {
    await sized(tester, const Size(1366, 1024), wrap(picker: FakePicker([])));

    final scan = tester.getSize(find.byKey(const Key('scan')));
    // Stretched across an iPad these read as a toolbar rather than a choice.
    expect(scan.width, lessThan(kReadableWidth));
    expect(scan.width, greaterThan(100), reason: 'still comfortably tappable');
  });

  testWidgets('a phone still gets full-width buttons', (tester) async {
    await sized(tester, const Size(390, 844), wrap(picker: FakePicker([])));

    final scan = tester.getSize(find.byKey(const Key('scan')));
    // Half the window, less the padding and the gap between the two.
    expect(scan.width, greaterThan(150));
  });

  testWidgets('the empty state keeps a readable measure on a wide window',
      (tester) async {
    await sized(tester, const Size(1366, 1024), wrap(picker: FakePicker([])));

    final body = find.textContaining('nothing is uploaded');
    expect(tester.getSize(body).width, lessThanOrEqualTo(kReadableWidth));
  });

  group('the viewfinder', () {
    Widget capture() => MaterialApp(
          theme: buildTheme(Brightness.dark),
          home: CaptureScreen(
            controller: FakeCamera(),
            permissions: FakePermissions(),
          ),
        );

    testWidgets('puts the shutter below the preview in portrait',
        (tester) async {
      await sized(tester, const Size(834, 1194), capture());
      await tester.pumpAndSettle();

      final preview = tester.getRect(find.byKey(const Key('fake-preview')));
      final shutter = tester.getRect(find.byKey(const Key('shutter')));
      expect(shutter.top, greaterThanOrEqualTo(preview.bottom));
    });

    testWidgets('puts it beside the preview in landscape', (tester) async {
      // Where a shutter pinned to the bottom edge would be furthest from
      // either hand, and would take height from the preview.
      await sized(tester, const Size(1194, 834), capture());
      await tester.pumpAndSettle();

      final preview = tester.getRect(find.byKey(const Key('fake-preview')));
      final shutter = tester.getRect(find.byKey(const Key('shutter')));
      expect(shutter.left, greaterThanOrEqualTo(preview.right));
      expect(shutter.center.dy, closeTo(preview.center.dy, 60));
    });
  });
}
