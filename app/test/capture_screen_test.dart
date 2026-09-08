import 'package:docscan/capture/capture_screen.dart';
import 'package:docscan/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'fakes.dart';

void main() {
  Widget wrap(FakeCamera camera, FakePermissions permissions) => MaterialApp(
        theme: buildTheme(Brightness.dark),
        home: CaptureScreen(controller: camera, permissions: permissions),
      );

  testWidgets('a granted permission opens the viewfinder', (tester) async {
    await tester.pumpWidget(wrap(FakeCamera(), FakePermissions()));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('fake-preview')), findsOneWidget);
    expect(find.byKey(const Key('shutter')), findsOneWidget);
  });

  testWidgets('a denied permission offers to ask again', (tester) async {
    final permissions = FakePermissions(granted: false);
    await tester.pumpWidget(wrap(FakeCamera(), permissions));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('capture-denied')), findsOneWidget);
    expect(find.text('Ask again'), findsOneWidget);
    // The explanation has to carry the reason, because "allow camera access"
    // with no reason is what a scanner asking to watch you looks like.
    expect(find.textContaining('never uploaded'), findsOneWidget);

    permissions.granted = true;
    await tester.tap(find.text('Ask again'));
    await tester.pumpAndSettle();

    expect(permissions.requests, 2);
    expect(find.byKey(const Key('fake-preview')), findsOneWidget);
  });

  testWidgets('a permanently denied permission sends you to Settings instead',
      (tester) async {
    // The distinction that matters: "Ask again" here is a button that
    // visibly does nothing, because the platform will not re-prompt.
    await tester.pumpWidget(
      wrap(
        FakeCamera(),
        FakePermissions(granted: false, permanentlyDenied: true),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('capture-denied-forever')), findsOneWidget);
    expect(find.text('Open settings'), findsOneWidget);
    expect(find.text('Ask again'), findsNothing);
  });

  testWidgets('a camera that will not start says so and offers a retry',
      (tester) async {
    await tester.pumpWidget(
      wrap(
        FakeCamera(failOnInitialise: 'This device reports no cameras.'),
        FakePermissions(),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('capture-failed')), findsOneWidget);
    expect(find.text('This device reports no cameras.'), findsOneWidget);
  });

  testWidgets('a double tap on the shutter takes one photograph, not two',
      (tester) async {
    final camera = FakeCamera(
      bytes: onePixelPng,
      captureDelay: const Duration(milliseconds: 80),
    );
    await tester.pumpWidget(wrap(camera, FakePermissions()));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('shutter')));
    await tester.pump(); // the guard is set, the capture is still in flight
    await tester.tap(find.byKey(const Key('shutter')));
    await tester.pumpAndSettle();

    expect(camera.captures, 1);
  });

  testWidgets('leaving the screen releases the camera', (tester) async {
    final camera = FakeCamera();
    await tester.pumpWidget(wrap(camera, FakePermissions()));
    await tester.pumpAndSettle();

    // A camera left open holds the hardware and drains the battery; on
    // Android it also stops any other app from opening it.
    await tester.pumpWidget(const MaterialApp(home: SizedBox()));
    await tester.pumpAndSettle();

    expect(camera.disposed, isTrue);
  });
}
