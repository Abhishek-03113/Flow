import 'package:flow_ui/core/theme/flow_theme.dart';
import 'package:flow_ui/core/widgets/toast.dart';
import 'package:flow_ui/data/mock_daemon_repository.dart';
import 'package:flow_ui/domain/daemon_command_exception.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/domain/pairing.dart';
import 'package:flow_ui/features/pairing/incoming_pairing_request_listener.dart';
import 'package:flow_ui/state/repository_providers.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

/// A mock daemon whose `respondToPairingRequest` always fails as if the
/// transport dropped between the prompt going up and the user answering.
class _DisconnectedOnRespondRepo extends MockDaemonRepository {
  @override
  Future<void> respondToPairingRequest(
    String requestId,
    PairingDecision decision,
  ) async {
    throw const DaemonCommandException(
      'daemon_disconnected',
      'lost connection',
    );
  }
}

void main() {
  testWidgets('shows dialog on request, Accept calls repo, null pops it', (
    tester,
  ) async {
    final repo = MockDaemonRepository();
    addTearDown(repo.dispose);
    final container = ProviderContainer(
      overrides: [daemonRepositoryProvider.overrideWithValue(repo)],
    );
    addTearDown(container.dispose);

    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: const MaterialApp(
          home: IncomingPairingRequestListener(
            child: Scaffold(body: SizedBox()),
          ),
        ),
      ),
    );
    await tester.pump();

    repo.simulateIncomingPairingRequest(
      deviceName: 'Windows Box',
      deviceOs: HostOs.windows,
    );
    await tester.pumpAndSettle();
    expect(find.text('Windows Box'), findsOneWidget);

    await tester.tap(find.text('Accept'));
    await tester.pumpAndSettle();

    // Mock cleared the stream and added the device. `.first` is read via
    // `runAsync` because a bare `await` on a stream never resolves inside
    // `testWidgets`' fake-async zone (same reason as
    // `test/e2e/daemon_ui_flow_e2e_test.dart`).
    final devices = await tester.runAsync(() => repo.watchDevices().first);
    expect(devices!.any((d) => d.name == 'Windows Box'), isTrue);
    expect(find.text('Windows Box'), findsNothing);
  });

  testWidgets('stream going null while open dismisses the dialog', (
    tester,
  ) async {
    final repo = MockDaemonRepository();
    addTearDown(repo.dispose);
    final container = ProviderContainer(
      overrides: [daemonRepositoryProvider.overrideWithValue(repo)],
    );
    addTearDown(container.dispose);

    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: const MaterialApp(
          home: IncomingPairingRequestListener(
            child: Scaffold(body: SizedBox()),
          ),
        ),
      ),
    );
    await tester.pump();

    repo.simulateIncomingPairingRequest(
      deviceName: 'Studio Linux',
      deviceOs: HostOs.linux,
    );
    await tester.pumpAndSettle();
    expect(find.text('Studio Linux'), findsOneWidget);

    // Someone else answered / it timed out on the daemon.
    final pending = await tester.runAsync(
      () => repo.watchIncomingPairingRequest().first,
    );
    await repo.respondToPairingRequest(
      pending!.requestId,
      PairingDecision.reject,
    );
    await tester.pumpAndSettle();
    expect(find.text('Studio Linux'), findsNothing);
  });

  testWidgets(
    'Accept when the daemon is unreachable: no uncaught error, toast shown',
    (tester) async {
      final repo = _DisconnectedOnRespondRepo();
      addTearDown(repo.dispose);
      final container = ProviderContainer(
        overrides: [daemonRepositoryProvider.overrideWithValue(repo)],
      );
      addTearDown(container.dispose);

      await tester.pumpWidget(
        UncontrolledProviderScope(
          container: container,
          child: MaterialApp(
            theme: FlowTheme.dark(),
            home: const IncomingPairingRequestListener(
              child: Scaffold(
                body: Stack(children: [SizedBox.expand(), ToastOverlay()]),
              ),
            ),
          ),
        ),
      );
      await tester.pump();

      repo.simulateIncomingPairingRequest(
        deviceName: 'Hostile Box',
        deviceOs: HostOs.windows,
      );
      await tester.pumpAndSettle();
      expect(find.text('Hostile Box'), findsOneWidget);

      await tester.tap(find.text('Accept'));
      // Let the dialog dismiss, the failing respond call reject, the catch
      // arm fire the toast, and the toast pill animate in — all well within
      // the toast's 1.7s auto-dismiss.
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
      await tester.pump(const Duration(milliseconds: 300));

      expect(tester.takeException(), isNull);
      expect(find.text('Hostile Box'), findsNothing);
      expect(find.text('lost connection'), findsOneWidget);

      // Drain the toast's own ~1.7s auto-dismiss timer so nothing outlives
      // the test.
      await tester.pump(const Duration(seconds: 2));
      await tester.pumpAndSettle();
    },
  );
}
