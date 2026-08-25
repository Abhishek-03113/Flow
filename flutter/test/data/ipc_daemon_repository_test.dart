import 'dart:convert';

import 'package:flow_ui/data/ipc_daemon_repository.dart';
import 'package:flow_ui/domain/daemon_link_state.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:stream_channel/stream_channel.dart';

/// Exercises [IpcDaemonRepository]'s event parsing and replay semantics
/// over an in-memory fake channel — no live `flow-daemon` process
/// required. The cross-language test against a *real* daemon is track D5
/// (`daemon/README.md`/`flutter/README.md`).
void main() {
  late StreamChannelController<dynamic> controller;
  late IpcDaemonRepository repo;

  setUp(() {
    controller = StreamChannelController<dynamic>();
    // Nothing in these tests sends a command yet (that's track D3), but
    // draining `foreign.stream` keeps `repo.dispose()`'s `sink.close()`
    // from hanging in tearDown — an unread `StreamChannelController` side
    // never signals done, unlike a real socket's peer.
    controller.foreign.stream.listen((_) {});
    repo = IpcDaemonRepository.withChannel(controller.local);
  });
  tearDown(() => repo.dispose());

  void sendEvent(String event, dynamic payload) {
    controller.foreign.sink.add(
      jsonEncode({'event': event, 'payload': payload}),
    );
  }

  final deviceJson = {
    'id': 'd2',
    'name': 'Work Laptop',
    'os': 'windows',
    'state': 'inactive',
    'last_seen': '2026-08-25T06:58:00Z',
  };

  test('devices_changed is parsed and delivered to a listener', () async {
    final future = repo.watchDevices().first;
    sendEvent('devices_changed', [deviceJson]);

    final devices = await future;
    expect(devices, hasLength(1));
    expect(devices.single.id, 'd2');
    expect(devices.single.os, HostOs.windows);
    expect(devices.single.state, DeviceState.inactive);
  });

  test('a subscriber joining after the event already arrived still sees it '
      '— the async*-vs-Stream.multi replay bug class the mock hit', () async {
    sendEvent('devices_changed', [deviceJson]);
    // Let the event actually propagate through _handleMessage before a
    // *new* listener subscribes — this is the case an async*-based
    // implementation can silently drop.
    await Future<void>.delayed(Duration.zero);

    final devices = await repo.watchDevices().first;
    expect(devices, hasLength(1));
    expect(devices.single.id, 'd2');
  });

  test(
    'every watch* stream replays its own last-seen event independently',
    () async {
      sendEvent('link_state_changed', 'permission_required');
      sendEvent('permission_changed', {
        'name': 'Accessibility access',
        'granted': false,
      });
      await Future<void>.delayed(Duration.zero);

      expect(
        await repo.watchLinkState().first,
        DaemonLinkState.permissionRequired,
      );
      final permission = await repo.watchPermission().first;
      expect(permission.granted, isFalse);
    },
  );

  test(
    'a later event on the same stream replaces the replayed value',
    () async {
      sendEvent('link_state_changed', 'connecting');
      await Future<void>.delayed(Duration.zero);
      sendEvent('link_state_changed', 'connected');
      await Future<void>.delayed(Duration.zero);

      expect(await repo.watchLinkState().first, DaemonLinkState.connected);
    },
  );
}
