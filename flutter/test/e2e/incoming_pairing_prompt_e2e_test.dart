import 'package:flow_ui/core/theme/flow_theme.dart';
import 'package:flow_ui/data/mock_daemon_repository.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/features/app_window/app_window_shell.dart';
import 'package:flow_ui/features/pairing/incoming_pairing_request_listener.dart';
import 'package:flow_ui/state/repository_providers.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets(
    'incoming pairing request → Accept adds the device to the devices list',
    (tester) async {
      final repo = MockDaemonRepository();
      addTearDown(repo.dispose);

      // Pump the app with IncomingPairingRequestListener wrapping AppWindowShell.
      await tester.pumpWidget(
        ProviderScope(
          overrides: [daemonRepositoryProvider.overrideWithValue(repo)],
          child: MaterialApp(
            debugShowCheckedModeBanner: false,
            theme: FlowTheme.light(),
            home: Scaffold(
              body: Center(
                child: IncomingPairingRequestListener(
                  child: AppWindowShell(
                    platform: HostOs.macos,
                    initialSection: AppSection.devices,
                  ),
                ),
              ),
            ),
          ),
        ),
      );

      // Bounded pump to settle the initial layout.
      for (var i = 0; i < 20; i++) {
        await tester.pump(const Duration(milliseconds: 50));
      }

      // Simulate an incoming pairing request.
      repo.simulateIncomingPairingRequest(
        deviceName: 'Nearby Laptop',
        deviceOs: HostOs.windows,
      );

      // Bounded pump to allow the dialog to appear.
      for (var i = 0; i < 20; i++) {
        await tester.pump(const Duration(milliseconds: 50));
      }

      // Assert the dialog is up with the device name and Accept button.
      expect(find.text('Nearby Laptop'), findsWidgets);
      expect(find.text('Accept'), findsOneWidget);

      // Tap Accept.
      await tester.tap(find.text('Accept'));

      // Bounded pump to process the acceptance.
      for (var i = 0; i < 20; i++) {
        await tester.pump(const Duration(milliseconds: 50));
      }

      // Assert the daemon side: the device was added.
      late List<Device> devices;
      await tester.runAsync(() async {
        devices = await repo.watchDevices().first;
      });
      expect(devices.any((d) => d.name == 'Nearby Laptop'), isTrue);

      // Assert the dialog is gone.
      expect(find.byType(AlertDialog), findsNothing);
    },
  );
}
