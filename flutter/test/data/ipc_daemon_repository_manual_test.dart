@Tags(['manual'])
library;

import 'package:flow_ui/data/ipc_daemon_repository.dart';
import 'package:flow_ui/domain/daemon_command_exception.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/domain/daemon_link_state.dart';
import 'package:flow_ui/domain/pairing.dart';
import 'package:flow_ui/domain/switch_key_binding.dart';
import 'package:flutter_test/flutter_test.dart';

/// Cross-language contract test (`daemon/todos.json` task D5): the same
/// 14 scenarios `mock_daemon_repository_test.dart` proves against
/// [MockDaemonRepository] run here against [IpcDaemonRepository]
/// connected to a **real** `flow-daemon` process — confirming the two
/// independently-built implementations are externally indistinguishable,
/// per `docs/contracts/README.md` ground rule 2.
///
/// ## Running it
///
/// Requires `flow-daemon` already running on `127.0.0.1:47823`, started
/// against a *fresh* database with `FLOW_DAEMON_SEED_MOCK_PARITY` set so
/// it seeds the mock-parity 3-device data this file assumes — a real
/// `flow-daemon` process never seeds that by default (a scratch `HOME`
/// guarantees the fresh database too, without touching your real one):
///
/// ```sh
/// # terminal 1, from the repo root
/// HOME=$(mktemp -d) FLOW_DAEMON_SEED_MOCK_PARITY=1 cargo run -p flow-daemon
///
/// # terminal 2
/// cd flutter
/// flutter test --tags manual --run-skipped test/data/ipc_daemon_repository_manual_test.dart
/// ```
///
/// Every test in this file shares the one running daemon's state — like
/// the daemon itself, and unlike `MockDaemonRepository` (fresh per Dart
/// test) — so re-running this file against the *same still-running*
/// `flow-daemon` without restarting it will fail assertions that assume
/// first-run seed state (e.g. "removeDevice drops a non-local device"
/// only passes once `d3` still exists). Restart `flow-daemon` (a fresh
/// `HOME`) between runs for a clean pass.
///
/// Tagged `manual` and skipped by default (`dart_test.yaml`) — a plain
/// `flutter test` never touches this file.
void main() {
  late IpcDaemonRepository repo;

  setUp(() => repo = IpcDaemonRepository());
  tearDown(() => repo.dispose());

  /// Waits for `stream` to emit a value matching `predicate`, replaying
  /// the current value first like any [ReplayChannel]-backed stream.
  ///
  /// A command's `Future` (e.g. [IpcDaemonRepository.resetSettings])
  /// resolves on its *ack*, which the real daemon sends before the
  /// corresponding `*_changed` *event* — reading `.first` immediately
  /// after `await`ing a command races that event's actual arrival over
  /// the network. `firstWhere` doesn't have that race: it keeps
  /// listening until the predicate matches, however many frames that
  /// takes — the same pattern real UI code reacting to these streams
  /// would use, not just a test-only workaround.
  Future<T> waitFor<T>(Stream<T> stream, bool Function(T value) predicate) {
    return stream.firstWhere(predicate).timeout(const Duration(seconds: 5));
  }

  test('watchDevices replays the seeded devices to a new listener', () async {
    final devices = await repo.watchDevices().first;
    expect(devices, hasLength(3));
    expect(devices.singleWhere((d) => d.id == 'd1').state, DeviceState.active);
    expect(
      devices.singleWhere((d) => d.id == 'd2').state,
      DeviceState.inactive,
    );
    expect(
      devices.singleWhere((d) => d.id == 'd3').state,
      DeviceState.disconnected,
    );
  });

  test('watchLinkState defaults to connected', () async {
    expect(await repo.watchLinkState().first, DaemonLinkState.connected);
  });

  test(
    'switchActiveDevice moves target to active and demotes the previous active',
    () async {
      await repo.switchActiveDevice('d2');
      final devices = await waitFor(
        repo.watchDevices(),
        (list) =>
            list.any((d) => d.id == 'd2' && d.state == DeviceState.active),
      );
      expect(
        devices.singleWhere((d) => d.id == 'd2').state,
        DeviceState.active,
      );
      expect(
        devices.singleWhere((d) => d.id == 'd1').state,
        DeviceState.inactive,
      );
    },
  );

  test('switchActiveDevice rejects a disconnected target', () async {
    await expectLater(
      repo.switchActiveDevice('d3'),
      throwsA(
        isA<DaemonCommandException>().having(
          (e) => e.code,
          'code',
          'device_not_switchable',
        ),
      ),
    );
  });

  test('switchActiveDevice rejects an unknown device', () async {
    await expectLater(
      repo.switchActiveDevice('nope'),
      throwsA(
        isA<DaemonCommandException>().having(
          (e) => e.code,
          'code',
          'device_not_found',
        ),
      ),
    );
  });

  test('removeDevice refuses to remove the local device', () async {
    await expectLater(
      repo.removeDevice('d1'),
      throwsA(
        isA<DaemonCommandException>().having(
          (e) => e.code,
          'code',
          'device_not_removable',
        ),
      ),
    );
  });

  test('removeDevice drops a non-local device', () async {
    await repo.removeDevice('d3');
    final devices = await waitFor(
      repo.watchDevices(),
      (list) => !list.any((d) => d.id == 'd3'),
    );
    expect(devices.any((d) => d.id == 'd3'), isFalse);
  });

  test(
    'pairing runs idle -> searching -> found -> requesting -> paired -> idle',
    () async {
      final stages = <PairingStage>[];
      final sub = repo.watchPairingSession().listen((s) => stages.add(s.stage));
      addTearDown(sub.cancel);

      await repo.startPairing();
      final found = await waitFor(
        repo.watchPairingSession(),
        (s) => s.stage == PairingStage.found,
      );
      expect(found.candidates, isNotEmpty);

      await repo.pairWithCandidate(found.candidates.first.id);
      final paired = await waitFor(
        repo.watchPairingSession(),
        (s) => s.stage == PairingStage.paired,
      );
      expect(paired.targetName, found.candidates.first.name);

      final devices = await waitFor(
        repo.watchDevices(),
        (list) => list.any((d) => d.name == paired.targetName),
      );
      expect(devices.any((d) => d.name == paired.targetName), isTrue);

      final idle = await waitFor(
        repo.watchPairingSession(),
        (s) => s.stage == PairingStage.idle,
      );
      expect(idle.stage, PairingStage.idle);

      expect(
        stages,
        containsAllInOrder([
          PairingStage.searching,
          PairingStage.found,
          PairingStage.requesting,
          PairingStage.paired,
          PairingStage.idle,
        ]),
      );
    },
    timeout: const Timeout(Duration(seconds: 15)),
  );

  test('cancelPairing resets a searching session to idle', () async {
    await repo.startPairing();
    await repo.cancelPairing();
    final session = await waitFor(
      repo.watchPairingSession(),
      (s) => s.stage == PairingStage.idle,
    );
    expect(session.stage, PairingStage.idle);
  });

  test('cancelPairing rejects when nothing is in progress', () async {
    await expectLater(
      repo.cancelPairing(),
      throwsA(
        isA<DaemonCommandException>().having(
          (e) => e.code,
          'code',
          'pairing_not_active',
        ),
      ),
    );
  });

  test('setSwitchKey updates settings', () async {
    await repo.setSwitchKey(SwitchKeyBinding.presets[2]);
    final settings = await waitFor(
      repo.watchSettings(),
      (s) => s.switchKey.label == 'F13',
    );
    expect(settings.switchKey.label, 'F13');
  });

  test('resetSettings restores defaults after a change', () async {
    await repo.setSwitchKey(SwitchKeyBinding.presets[1]);
    await repo.resetSettings();
    final settings = await waitFor(
      repo.watchSettings(),
      (s) => s.switchKey.label == SwitchKeyBinding.defaultBinding.label,
    );
    expect(settings.switchKey.label, SwitchKeyBinding.defaultBinding.label);
  });

  test('requestPermission grants and then rejects a second request', () async {
    expect((await repo.watchPermission().first).granted, isFalse);
    await repo.requestPermission();
    final permission = await waitFor(repo.watchPermission(), (p) => p.granted);
    expect(permission.granted, isTrue);
    await expectLater(
      repo.requestPermission(),
      throwsA(
        isA<DaemonCommandException>().having(
          (e) => e.code,
          'code',
          'permission_already_granted',
        ),
      ),
    );
  });

  // `retryConnection`'s success path (disconnected/error -> connecting)
  // needs the real daemon's link state actually off `connected`, which
  // requires a second paired daemon dropping its link — outside this
  // single-process, freshly-seeded scenario. Only the precondition
  // rejection is exercised here; `mock_daemon_repository_test.dart`
  // covers the success path via `debugSetLinkState`, and
  // `daemon/tests/service_parity.rs` covers it directly against
  // `DaemonService`.
  test('retryConnection rejects when the link is already connected', () async {
    expect(await repo.watchLinkState().first, DaemonLinkState.connected);
    await expectLater(
      repo.retryConnection(),
      throwsA(
        isA<DaemonCommandException>().having(
          (e) => e.code,
          'code',
          'link_not_recoverable',
        ),
      ),
    );
  });
}
