import 'package:flow_ui/data/mock_daemon_repository.dart';
import 'package:flow_ui/domain/daemon_command_exception.dart';
import 'package:flow_ui/domain/daemon_link_state.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/domain/pairing.dart';
import 'package:flow_ui/domain/switch_key_binding.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  late MockDaemonRepository repo;

  setUp(() => repo = MockDaemonRepository());
  tearDown(() => repo.dispose());

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

  test(
    'watchLinkState defaults to connected and reflects debugSetLinkState',
    () async {
      expect(await repo.watchLinkState().first, DaemonLinkState.connected);
      final next = repo.watchLinkState().skip(1).first;
      repo.debugSetLinkState(DaemonLinkState.reconnecting);
      expect(await next, DaemonLinkState.reconnecting);
    },
  );

  test(
    'switchActiveDevice moves target to active and demotes the previous active',
    () async {
      await repo.switchActiveDevice('d2');
      final devices = await repo.watchDevices().first;
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
    final devices = await repo.watchDevices().first;
    expect(devices.any((d) => d.id == 'd3'), isFalse);
  });

  test(
    'pairing runs idle -> searching -> found -> requesting -> paired -> idle',
    () async {
      final stages = <PairingStage>[];
      final sub = repo.watchPairingSession().listen((s) => stages.add(s.stage));
      addTearDown(sub.cancel);

      await repo.startPairing();
      await Future<void>.delayed(const Duration(milliseconds: 1300));
      final found = await repo.watchPairingSession().first;
      expect(found.stage, PairingStage.found);
      expect(found.candidates, isNotEmpty);

      await repo.pairWithCandidate(found.candidates.first.id);
      await Future<void>.delayed(const Duration(milliseconds: 1600));
      final paired = await repo.watchPairingSession().first;
      expect(paired.stage, PairingStage.paired);
      expect(paired.targetName, found.candidates.first.name);

      final devices = await repo.watchDevices().first;
      expect(devices.any((d) => d.name == paired.targetName), isTrue);

      await Future<void>.delayed(const Duration(milliseconds: 1700));
      expect((await repo.watchPairingSession().first).stage, PairingStage.idle);

      expect(stages.first, PairingStage.idle);
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
    timeout: const Timeout(Duration(seconds: 10)),
  );

  test('cancelPairing resets a searching session to idle', () async {
    await repo.startPairing();
    await repo.cancelPairing();
    expect((await repo.watchPairingSession().first).stage, PairingStage.idle);
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
    final settings = await repo.watchSettings().first;
    expect(settings.switchKey.label, 'F13');
  });

  test('resetSettings restores defaults after a change', () async {
    await repo.setSwitchKey(SwitchKeyBinding.presets[1]);
    await repo.resetSettings();
    final settings = await repo.watchSettings().first;
    expect(settings.switchKey.label, SwitchKeyBinding.defaultBinding.label);
  });

  test('requestPermission grants and then rejects a second request', () async {
    expect((await repo.watchPermission().first).granted, isFalse);
    await repo.requestPermission();
    expect((await repo.watchPermission().first).granted, isTrue);
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
}
