import 'package:flutter/widgets.dart';

/// How wide the window is, in the only three sizes this app treats differently.
///
/// Deliberately a property of the *window*, not the device. An iPad in Split
/// View hands the app a 320-point-wide window, and an app that asked "is this
/// an iPad?" would lay out for a tablet inside a column narrower than an
/// iPhone's. iPadOS multitasking makes that an everyday state rather than an
/// edge case, so the question is never asked.
enum Pane {
  /// A phone, or an iPad window narrowed to a phone's width.
  compact,

  /// A large phone in landscape, or an iPad in half of Split View.
  medium,

  /// A full-screen iPad.
  expanded;

  static Pane of(BuildContext context) => forWidth(MediaQuery.sizeOf(context).width);

  static Pane forWidth(double width) {
    if (width < 600) return Pane.compact;
    if (width < 1000) return Pane.medium;
    return Pane.expanded;
  }

  bool get isCompact => this == Pane.compact;
}

/// The widest a column of prose or controls is allowed to get.
///
/// Not the window width. A line of text spanning a 12.9-inch iPad is about 140
/// characters, roughly twice the length the eye tracks back from comfortably,
/// and a pair of buttons stretched to that width reads as a toolbar rather
/// than a choice.
const double kReadableWidth = 640;

/// The widest the whole content area gets before it stops growing and centres.
const double kContentWidth = 1120;

/// A result card's ideal width. The grid fits as many of these as the window
/// allows, so the count follows the window rather than a device guess: one on
/// a phone, two in Split View, three or four full-screen.
const double kCardWidth = 420;

/// Centres its child and stops it growing past [max].
class Constrained extends StatelessWidget {
  const Constrained({super.key, required this.child, this.max = kContentWidth});

  final Widget child;
  final double max;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: ConstrainedBox(
        constraints: BoxConstraints(maxWidth: max),
        child: child,
      ),
    );
  }
}
