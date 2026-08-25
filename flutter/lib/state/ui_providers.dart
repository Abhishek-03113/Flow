import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Pure UI state that has no daemon counterpart — never merged into the
/// same provider as anything from [DaemonRepository] (`repository_
/// providers.dart`), so daemon-derived and local-only state stay
/// separable at a glance.
///
/// [ThemeMode.system] defers to the OS; the design system otherwise only
/// distinguishes light/dark (`docs/contracts` has no notion of theme —
/// it's UI-only).
final themeModeProvider = StateProvider<ThemeMode>((ref) => ThemeMode.dark);

/// Whether the tray popover is open. Lives here rather than as
/// widget-local state because both the tray icon (toggling it) and
/// external triggers (opening the dashboard, which closes the popover)
/// need to observe/drive it.
final trayOpenProvider = StateProvider<bool>((ref) => true);
