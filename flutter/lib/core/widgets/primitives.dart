import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../theme/tokens.dart';

/// A device/link status indicator (`todos.json` `sharedDesignTokens.
/// statusDotShape`): a solid filled+glow circle for a definite state
/// (active, error), or a ring for a "reachable but not current" state
/// (inactive, disconnected). [pulse] adds the `cd-breathe` opacity pulse
/// used for connecting/reconnecting/switching.
class StatusDot extends StatelessWidget {
  const StatusDot({
    super.key,
    required this.color,
    this.filled = true,
    this.pulse = false,
    this.size = 9,
  });

  final Color color;
  final bool filled;
  final bool pulse;
  final double size;

  @override
  Widget build(BuildContext context) {
    final dot = Container(
      width: size,
      height: size,
      decoration: BoxDecoration(
        shape: BoxShape.circle,
        color: filled ? color : Colors.transparent,
        border: filled ? null : Border.all(color: color, width: 1.5),
        boxShadow: filled
            ? [BoxShadow(color: color.withValues(alpha: 0.4), blurRadius: 8)]
            : null,
      ),
    );
    if (!pulse) return dot;
    return _Breathe(child: dot);
  }
}

class _Breathe extends StatefulWidget {
  const _Breathe({required this.child});

  final Widget child;

  @override
  State<_Breathe> createState() => _BreatheState();
}

class _BreatheState extends State<_Breathe>
    with SingleTickerProviderStateMixin {
  late final _controller = AnimationController(
    vsync: this,
    duration: FlowMotion.breathe,
  )..repeat(reverse: true);

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return FadeTransition(
      opacity: _controller
          .drive(CurveTween(curve: Curves.easeInOut))
          .drive(Tween(begin: 1.0, end: 0.4)),
      child: widget.child,
    );
  }
}

/// The pill-track toggle from the design (38x22px track, 18px thumb).
class FlowToggle extends StatelessWidget {
  const FlowToggle({
    super.key,
    required this.value,
    required this.onChanged,
    required this.activeColor,
    required this.trackColor,
  });

  final bool value;
  final ValueChanged<bool> onChanged;
  final Color activeColor;
  final Color trackColor;

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: () => onChanged(!value),
      child: AnimatedContainer(
        duration: FlowMotion.hoverPress,
        curve: FlowMotion.ease,
        width: 38,
        height: 22,
        padding: const EdgeInsets.all(2),
        decoration: BoxDecoration(
          color: value ? activeColor : trackColor,
          borderRadius: BorderRadius.circular(FlowRadii.toggleTrack),
        ),
        alignment: value ? Alignment.centerRight : Alignment.centerLeft,
        child: Container(
          width: 18,
          height: 18,
          decoration: const BoxDecoration(
            color: Colors.white,
            shape: BoxShape.circle,
            boxShadow: [
              BoxShadow(
                color: Color(0x40000000),
                blurRadius: 3,
                offset: Offset(0, 1),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// One option in a [FlowSegmentedControl].
class FlowSegment<T> {
  const FlowSegment({required this.value, required this.label});

  final T value;
  final String label;
}

/// The pill-group selector used for View/Connection/Platform/Theme
/// pickers and the Appearance/Sensitivity settings rows.
class FlowSegmentedControl<T> extends StatelessWidget {
  const FlowSegmentedControl({
    super.key,
    required this.segments,
    required this.selected,
    required this.onChanged,
    required this.background,
    required this.selectedBackground,
    required this.selectedForeground,
    required this.unselectedForeground,
  });

  final List<FlowSegment<T>> segments;
  final T selected;
  final ValueChanged<T> onChanged;
  final Color background;
  final Color selectedBackground;
  final Color selectedForeground;
  final Color unselectedForeground;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(2),
      decoration: BoxDecoration(
        color: background,
        borderRadius: BorderRadius.circular(8),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final segment in segments)
            GestureDetector(
              onTap: () => onChanged(segment.value),
              child: AnimatedContainer(
                duration: FlowMotion.hoverPress,
                curve: FlowMotion.ease,
                padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 4),
                decoration: BoxDecoration(
                  color: segment.value == selected
                      ? selectedBackground
                      : Colors.transparent,
                  borderRadius: BorderRadius.circular(6),
                ),
                child: Text(
                  segment.label,
                  style: FlowType.buttonLabel(
                    segment.value == selected
                        ? selectedForeground
                        : unselectedForeground,
                    weight: FontWeight.w600,
                  ).copyWith(fontSize: 11.5),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

enum FlowButtonKind { primary, ghost, danger }

/// Buttons in the three kinds the design uses: primary (accent-filled,
/// onboarding CTAs), ghost (subtle `mat1` fill, secondary actions), and
/// danger (`dangerSoft` fill, destructive actions).
class FlowButton extends StatelessWidget {
  const FlowButton({
    super.key,
    required this.label,
    required this.onPressed,
    required this.kind,
    required this.background,
    required this.foreground,
    this.large = false,
  });

  final String label;
  final VoidCallback? onPressed;
  final FlowButtonKind kind;
  final Color background;
  final Color foreground;

  /// Onboarding-style CTAs use larger padding/type than inline buttons.
  final bool large;

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onPressed,
      child: AnimatedContainer(
        duration: FlowMotion.hoverPress,
        curve: FlowMotion.ease,
        padding: large
            ? const EdgeInsets.symmetric(horizontal: 20, vertical: 9)
            : const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        decoration: BoxDecoration(
          color: background,
          borderRadius: BorderRadius.circular(large ? 9 : FlowRadii.button),
        ),
        child: Text(
          label,
          style: FlowType.buttonLabel(
            foreground,
            weight: large ? FontWeight.w700 : FontWeight.w600,
          ).copyWith(fontSize: large ? 13 : 12),
        ),
      ),
    );
  }
}

/// A small chip-style button (switch-key presets).
class FlowChip extends StatelessWidget {
  const FlowChip({
    super.key,
    required this.label,
    required this.onPressed,
    required this.selected,
    required this.selectedColor,
    required this.background,
    required this.selectedForeground,
    required this.foreground,
  });

  final String label;
  final VoidCallback onPressed;
  final bool selected;
  final Color selectedColor;
  final Color background;
  final Color selectedForeground;
  final Color foreground;

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onPressed,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
        decoration: BoxDecoration(
          color: selected ? selectedColor : background,
          borderRadius: BorderRadius.circular(9),
        ),
        child: Text(
          label,
          style: FlowType.buttonLabel(
            selected ? selectedForeground : foreground,
          ).copyWith(fontSize: 12),
        ),
      ),
    );
  }
}

/// The rotating-ring loading indicator used during searching/requesting.
class FlowSpinner extends StatefulWidget {
  const FlowSpinner({
    super.key,
    required this.color,
    required this.trackColor,
    this.size = 20,
  });

  final Color color;
  final Color trackColor;
  final double size;

  @override
  State<FlowSpinner> createState() => _FlowSpinnerState();
}

class _FlowSpinnerState extends State<FlowSpinner>
    with SingleTickerProviderStateMixin {
  late final _controller = AnimationController(
    vsync: this,
    duration: FlowMotion.spin,
  )..repeat();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return RotationTransition(
      turns: _controller,
      child: CustomPaint(
        size: Size.square(widget.size),
        painter: _SpinnerPainter(
          color: widget.color,
          trackColor: widget.trackColor,
        ),
      ),
    );
  }
}

class _SpinnerPainter extends CustomPainter {
  _SpinnerPainter({required this.color, required this.trackColor});

  final Color color;
  final Color trackColor;

  @override
  void paint(Canvas canvas, Size size) {
    final center = size.center(Offset.zero);
    final radius = size.width / 2 - 1;
    final track = Paint()
      ..color = trackColor
      ..style = PaintingStyle.stroke
      ..strokeWidth = 2;
    canvas.drawCircle(center, radius, track);

    final arc = Paint()
      ..color = color
      ..style = PaintingStyle.stroke
      ..strokeWidth = 2
      ..strokeCap = StrokeCap.round;
    canvas.drawArc(
      Rect.fromCircle(center: center, radius: radius),
      -math.pi / 2,
      math.pi / 2,
      false,
      arc,
    );
  }

  @override
  bool shouldRepaint(covariant _SpinnerPainter oldDelegate) =>
      color != oldDelegate.color || trackColor != oldDelegate.trackColor;
}
