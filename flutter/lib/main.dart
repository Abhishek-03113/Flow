import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:window_manager/window_manager.dart';

import 'app.dart';
import 'state/ui_mode.dart';

void main() {
  // A missing/unreachable `flow-daemon` (e.g. `flutter run` without first
  // starting the daemon) surfaces as an async `WebSocketChannelException`
  // out of `IpcDaemonRepository`'s connection attempt. That failure is
  // meant to land in a provider's `AsyncError` so the UI can show it — not
  // escape as an unhandled isolate error and take the whole app down.
  // `runZonedGuarded` is the last line of defense for anything that still
  // gets away, matching the "degrade gracefully, not fatally" contract
  // already used for the tray/window platform channels below.
  runZonedGuarded(_start, (error, stackTrace) {
    debugPrint('Unhandled error: $error\n$stackTrace');
  });
}

Future<void> _start() async {
  WidgetsFlutterBinding.ensureInitialized();
  FlutterError.onError = (details) {
    FlutterError.presentError(details);
    debugPrint('FlutterError: ${details.exceptionAsString()}');
  };

  // The dev harness (`--dart-define=FLOW_UI_MODE=harness`, see
  // `app.dart`) renders its own mock desktop/platform comparison views
  // and doesn't want a real OS window shape imposed on it — only the
  // shipped app flow docks a real window.
  if (uiMode == UiMode.app) {
    try {
      await windowManager.ensureInitialized();
      const windowOptions = WindowOptions(
        size: Size(760, 560),
        minimumSize: Size(760, 560),
        center: true,
        title: 'Flow',
      );
      await windowManager.waitUntilReadyToShow(windowOptions, () async {
        await windowManager.show();
        await windowManager.focus();
      });
    } catch (error, stackTrace) {
      // No native `window_manager` implementation available in this
      // environment — fall through and let the app render in whatever
      // window Flutter already has, rather than dying before `runApp`.
      debugPrint('window_manager init failed: $error\n$stackTrace');
    }
  }

  runApp(const ProviderScope(child: FlowApp()));
}
