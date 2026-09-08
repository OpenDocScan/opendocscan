import 'package:docscan/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// The design system's two rules that are easy to get backwards, as tests.
///
/// A comment saying "brass is never an affordance" is worth less than a test
/// that fails when someone makes the shutter brass, because the failure mode
/// is invisible: the app still works, it just stops looking like the family.
void main() {
  test('the accent is the product mark, never the action colour', () {
    for (final theme in [DocScanTheme.dark, DocScanTheme.light]) {
      expect(
        theme.accent,
        isNot(theme.brand),
        reason: 'green means action; brass identifies the product',
      );
    }
  });

  test('the action colour is the suite brand green, shared by every product',
      () {
    expect(DocScanTheme.dark.brand, DocScanTokens.green);
    expect(DocScanTheme.light.brand, DocScanTokens.green);
  });

  test('the accent is accent slot 20, lifted only where it must be', () {
    // Brass is 3.4:1 on the dark ground and 5.0:1 on the light one, so dark
    // mode uses the lifted value and light mode uses the slot itself. Getting
    // this backwards makes the wordmark's dot vanish, which is exactly what
    // happened on the website before it was measured.
    expect(DocScanTheme.dark.accent, DocScanTokens.brassLifted);
    expect(DocScanTheme.light.accent, DocScanTokens.brass);
  });

  testWidgets('the primary button is the brand colour in both themes',
      (tester) async {
    for (final brightness in [Brightness.dark, Brightness.light]) {
      final theme = buildTheme(brightness);
      final style = theme.filledButtonTheme.style!;
      final background =
          style.backgroundColor!.resolve(<WidgetState>{}) as Color;

      expect(background, DocScanTokens.green, reason: '$brightness');
    }
  });

  testWidgets('the wordmark is three parts, and the dot carries the accent',
      (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: buildTheme(Brightness.dark),
        home: const Scaffold(body: Wordmark()),
      ),
    );

    final text = tester.widget<Text>(find.byType(Text));
    final spans = (text.textSpan! as TextSpan).children!.cast<TextSpan>();

    expect(spans.map((s) => s.text), ['Open', 'DocScan', '.']);
    expect(spans[0].style!.color, DocScanTheme.dark.textMuted);
    expect(spans[1].style!.color, DocScanTheme.dark.textStrong);
    expect(spans[2].style!.color, DocScanTheme.dark.accent);
  });
}
