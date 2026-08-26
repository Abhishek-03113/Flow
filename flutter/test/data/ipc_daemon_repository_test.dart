import 'dart:async';
import 'dart:convert';

import 'package:flow_ui/data/ipc_daemon_repository.dart';
import 'package:flow_ui/domain/daemon_command_exception.dart';
import 'package:flow_ui/domain/daemon_link_state.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/domain/settings.dart';
import 'package:flow_ui/domain/switch_key_binding.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:stream_channel/stream_channel.dart';

/// Exercises [IpcDaemonRepository]'s event parsing/replay semantics and
/// command round-trips over an in-memory fake channel — no live
/// `flow-daemon` process required. The cross-language test against a
/// *real* daemon is track D5 (`daemon/README.md`/`flutter/README.md`).
void main() {
  late StreamChannelController<dynamic> controller;
  late StreamController<dynamic> sentFeed;
  late IpcDaemonRepository repo;

  setUp(() {
    controller = StreamChannelController<dynamic>();
    // Feeding a broadcast controller (rather than reading
    // `foreign.stream` directly in each command test) keeps
    // `foreign.stream` continuously listened-to — required for
    // `repo.dispose()`'s `sink.close()` to resolve in tearDown, since an
    // unread `StreamChannelController` side never signals done the way a
    // real socket's peer does — while still letting `nextRequest()`
    // below observe each sent request via its own `.first`.
    sentFeed = StreamController<dynamic>.broadcast();
    controller.foreign.stream.listen(sentFeed.add);
    repo = IpcDaemonRepository.withChannel(controller.local);
  });
  tearDown(() async {
    await repo.dispose();
    await sentFeed.close();
  });

  void sendEvent(String event, dynamic payload) {
    controller.foreign.sink.add(
      jsonEncode({'event': event, 'payload': payload}),
    );
  }

  Future<Map<String, dynamic>> nextRequest() async {
    final raw = await sentFeed.stream.first;
    return jsonDecode(raw as String) as Map<String, dynamic>;
  }

  void reply(String id, {Object? error}) {
    controller.foreign.sink.add(
      jsonEncode(
        error == null
            ? {'id': id, 'ok': true}
            : {'id': id, 'ok': false, 'error': error},
      ),
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

  test(
    'switchActiveDevice sends the right request and resolves on ack',
    () async {
      final future = repo.switchActiveDevice('d2');
      final request = await nextRequest();
      expect(request['command'], 'switch_active_device');
      expect(request['payload'], {'device_id': 'd2'});

      reply(request['id'] as String);
      await future; // completes without throwing
    },
  );

  test('a command with no payload sends null, not an empty object', () async {
    final future = repo.startPairing();
    final request = await nextRequest();
    expect(request['command'], 'start_pairing');
    expect(request['payload'], isNull);

    reply(request['id'] as String);
    await future;
  });

  test('setSwitchKey sends the binding as the payload directly', () async {
    final future = repo.setSwitchKey(
      const SwitchKeyBinding(label: 'F13', keys: ['F13']),
    );
    final request = await nextRequest();
    expect(request['command'], 'set_switch_key');
    expect(request['payload'], {
      'label': 'F13',
      'keys': ['F13'],
    });

    reply(request['id'] as String);
    await future;
  });

  test('updateSettings sends only the patched fields', () async {
    final future = repo.updateSettings(const SettingsPatch(shareMouse: false));
    final request = await nextRequest();
    expect(request['command'], 'update_settings');
    expect(request['payload'], {'share_mouse': false});

    reply(request['id'] as String);
    await future;
  });

  test(
    'a rejected command throws DaemonCommandException with the daemon\'s code',
    () async {
      final future = repo.removeDevice('d1');
      final request = await nextRequest();

      reply(
        request['id'] as String,
        error: {'code': 'device_not_removable', 'message': 'nope'},
      );

      await expectLater(
        future,
        throwsA(
          isA<DaemonCommandException>()
              .having((e) => e.code, 'code', 'device_not_removable')
              .having((e) => e.message, 'message', 'nope'),
        ),
      );
    },
  );

  /// Regression: [IpcDaemonRepository]'s frame handler runs inside a
  /// [StreamSubscription] callback, so a throw there escapes as an
  /// unhandled async error and tears the subscription down — one bad
  /// frame used to silently kill every `watch*` stream for the rest of
  /// the process's life, not just drop that frame.
  group('a malformed frame is dropped, not fatal to the connection', () {
    Future<void> expectStillLive() async {
      final future = repo.watchDevices().first;
      sendEvent('devices_changed', [deviceJson]);
      expect((await future).single.id, 'd2');
    }

    test('non-JSON text', () async {
      controller.foreign.sink.add('not json at all');
      await expectStillLive();
    });

    test('valid JSON that is not an envelope object', () async {
      controller.foreign.sink.add(jsonEncode([1, 2, 3]));
      await expectStillLive();
    });

    test('a known event carrying an unparseable payload', () async {
      sendEvent('devices_changed', 'not a device list');
      await expectStillLive();
    });

    test('an enum variant this build does not know', () async {
      sendEvent('link_state_changed', 'a_state_from_a_newer_daemon');
      await expectStillLive();
    });
  });

  test('each request gets a fresh id, correlated independently', () async {
    final firstFuture = repo.startPairing();
    final firstRequest = await nextRequest();
    final secondFuture = repo.cancelPairing();
    final secondRequest = await nextRequest();

    expect(firstRequest['id'], isNot(secondRequest['id']));

    // Reply out of order — resolution is keyed by id, not send order.
    reply(secondRequest['id'] as String);
    reply(firstRequest['id'] as String);
    await firstFuture;
    await secondFuture;
  });
}
