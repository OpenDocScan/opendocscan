import 'package:flutter/material.dart';

/// The design tokens, as Dart.
///
/// These are transcribed from `tokens/css/colors.css` in the monorepo — the
/// same set the website and the web app are built from. Transcribed rather
/// than shared, because Flutter cannot read a CSS custom property, and that
/// makes this the one file in the app allowed to name a colour. Everything
/// else reads [DocScanTheme].
///
/// Two rules from the design system that are easy to get backwards, so they
/// are encoded here rather than left to each widget:
///
/// * **Green means action.** The shutter, the primary button, a selected chip.
///   It is the suite's brand colour, shared by every product.
/// * **Brass identifies the product and is never an affordance.** It is accent
///   slot 20, it appears on the mark and the wordmark's full stop, and it is
///   never the fill of something you can press. It is also 3.4:1 as text on the
///   dark ground, so small type uses [brassLifted].
class DocScanTokens {
  const DocScanTokens._();

  // Layer 1 — the brand palette, verbatim.
  static const black = Color(0xFF000000);
  static const white = Color(0xFFFFFFFF);
  static const green = Color(0xFF00C896);
  static const red = Color(0xFFFF4D4D);
  static const gray200 = Color(0xFFDEDED9);
  static const gray600 = Color(0xFF656565);
  static const gray900 = Color(0xFF282828);
  static const gray950 = Color(0xFF1A1A1A);

  static const backgroundLight = Color(0xFFF2F2F2);
  static const backgroundDark = Color(0xFF111111);
  static const fontColor = Color(0xFF020202);

  /// Accent slot 20. `--yellow` at 44% black; see `tokens/ACCENT-SLOTS.md`.
  static const brass = Color(0xFF6E6929);

  /// Brass lifted 32% toward white — 6.3:1 on the dark ground, which brass
  /// itself is not. Same value the website uses for the wordmark's dot.
  static const brassLifted = Color(0xFF9A976D);
}

/// The semantic layer. Widgets read these, never the palette above.
@immutable
class DocScanTheme extends ThemeExtension<DocScanTheme> {
  const DocScanTheme({
    required this.page,
    required this.surface,
    required this.surfaceRaised,
    required this.hairline,
    required this.textStrong,
    required this.textMuted,
    required this.accent,
    required this.brand,
    required this.brandContrast,
    required this.danger,
  });

  final Color page;
  final Color surface;
  final Color surfaceRaised;
  final Color hairline;
  final Color textStrong;
  final Color textMuted;

  /// The product's own colour. Identity only — never a button.
  final Color accent;

  /// Action. Every pressable primary control.
  final Color brand;
  final Color brandContrast;
  final Color danger;

  static const dark = DocScanTheme(
    page: DocScanTokens.backgroundDark,
    surface: DocScanTokens.gray950,
    surfaceRaised: DocScanTokens.gray900,
    hairline: Color(0x1AFFFFFF),
    textStrong: DocScanTokens.white,
    textMuted: Color(0x99FFFFFF),
    accent: DocScanTokens.brassLifted,
    brand: DocScanTokens.green,
    brandContrast: DocScanTokens.black,
    danger: DocScanTokens.red,
  );

  static const light = DocScanTheme(
    page: DocScanTokens.backgroundLight,
    surface: DocScanTokens.white,
    surfaceRaised: DocScanTokens.white,
    hairline: DocScanTokens.gray200,
    textStrong: DocScanTokens.fontColor,
    textMuted: DocScanTokens.gray600,
    accent: DocScanTokens.brass,
    brand: DocScanTokens.green,
    brandContrast: DocScanTokens.black,
    danger: DocScanTokens.red,
  );

  static DocScanTheme of(BuildContext context) =>
      Theme.of(context).extension<DocScanTheme>() ?? dark;

  @override
  DocScanTheme copyWith({
    Color? page,
    Color? surface,
    Color? surfaceRaised,
    Color? hairline,
    Color? textStrong,
    Color? textMuted,
    Color? accent,
    Color? brand,
    Color? brandContrast,
    Color? danger,
  }) {
    return DocScanTheme(
      page: page ?? this.page,
      surface: surface ?? this.surface,
      surfaceRaised: surfaceRaised ?? this.surfaceRaised,
      hairline: hairline ?? this.hairline,
      textStrong: textStrong ?? this.textStrong,
      textMuted: textMuted ?? this.textMuted,
      accent: accent ?? this.accent,
      brand: brand ?? this.brand,
      brandContrast: brandContrast ?? this.brandContrast,
      danger: danger ?? this.danger,
    );
  }

  @override
  DocScanTheme lerp(ThemeExtension<DocScanTheme>? other, double t) {
    if (other is! DocScanTheme) return this;
    return DocScanTheme(
      page: Color.lerp(page, other.page, t)!,
      surface: Color.lerp(surface, other.surface, t)!,
      surfaceRaised: Color.lerp(surfaceRaised, other.surfaceRaised, t)!,
      hairline: Color.lerp(hairline, other.hairline, t)!,
      textStrong: Color.lerp(textStrong, other.textStrong, t)!,
      textMuted: Color.lerp(textMuted, other.textMuted, t)!,
      accent: Color.lerp(accent, other.accent, t)!,
      brand: Color.lerp(brand, other.brand, t)!,
      brandContrast: Color.lerp(brandContrast, other.brandContrast, t)!,
      danger: Color.lerp(danger, other.danger, t)!,
    );
  }
}

ThemeData buildTheme(Brightness brightness) {
  final tokens =
      brightness == Brightness.dark ? DocScanTheme.dark : DocScanTheme.light;

  return ThemeData(
    brightness: brightness,
    scaffoldBackgroundColor: tokens.page,
    colorScheme: ColorScheme.fromSeed(
      seedColor: tokens.brand,
      brightness: brightness,
    ).copyWith(
      primary: tokens.brand,
      onPrimary: tokens.brandContrast,
      surface: tokens.surface,
      error: tokens.danger,
    ),
    extensions: [tokens],
    appBarTheme: AppBarTheme(
      backgroundColor: tokens.page,
      foregroundColor: tokens.textStrong,
      elevation: 0,
      centerTitle: true,
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        backgroundColor: tokens.brand,
        foregroundColor: tokens.brandContrast,
        minimumSize: const Size.fromHeight(52),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(14),
        ),
        textStyle: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        foregroundColor: tokens.textStrong,
        minimumSize: const Size.fromHeight(52),
        side: BorderSide(color: tokens.hairline),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(14),
        ),
        textStyle: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
      ),
    ),
  );
}

/// The family wordmark: "Open" muted, the product word at full strength, then
/// a full stop in the product's accent. The same lockup as the website and the
/// web app, and the only place brass appears in this app.
class Wordmark extends StatelessWidget {
  const Wordmark({super.key});

  @override
  Widget build(BuildContext context) {
    final theme = DocScanTheme.of(context);
    const style = TextStyle(
      fontSize: 19,
      fontWeight: FontWeight.w600,
      letterSpacing: -0.6,
    );

    return Text.rich(
      TextSpan(
        children: [
          TextSpan(text: 'Open', style: style.copyWith(color: theme.textMuted)),
          TextSpan(
            text: 'DocScan',
            style: style.copyWith(color: theme.textStrong),
          ),
          TextSpan(text: '.', style: style.copyWith(color: theme.accent)),
        ],
      ),
    );
  }
}
