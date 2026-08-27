import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:window_manager/window_manager.dart';

import 'app.dart';
import 'state/ui_mode.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // The dev harness (`--dart-define=FLOW_UI_MODE=harness`, see
  // `app.dart`) renders its own mock desktop/platform comparison views
  // and doesn't want a real OS window shape imposed on it — only the
  // shipped app flow docks a real window.
  if (uiMode == UiMode.app) {
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
  }

  runApp(const ProviderScope(child: FlowApp()));
}
