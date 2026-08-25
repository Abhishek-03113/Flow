import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/theme/tokens.dart';
import '../../../core/widgets/primitives.dart';
import '../../../domain/daemon_command_exception.dart';
import '../../../domain/device.dart';
import '../../../domain/settings.dart';
import '../../../state/repository_providers.dart';
import '../../../state/ui_providers.dart';
import '_setting_row.dart';

/// The Devices settings section: the full paired-device list with "last
/// seen" meta and removal, plus reconnect/auto-connect toggles.
class DevicesSection extends ConsumerWidget {
  const DevicesSection({super.key, required this.palette});

  final FlowPalette palette;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = palette;
    final devices = ref.watch(devicesProvider).valueOrNull ?? const <Device>[];
    final settings = ref.watch(settingsProvider).valueOrNull;

    void patch(SettingsPatch patch) =>
        ref.read(daemonRepositoryProvider).updateSettings(patch);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          'Devices & connection',
          style: FlowType.body(
            c.text1,
            weight: FontWeight.w700,
          ).copyWith(fontSize: 15),
        ),
        const SizedBox(height: 8),
        for (final device in devices)
          _DeviceLastSeenRow(palette: c, device: device),
        const SizedBox(height: 8),
        SettingRow(
          palette: c,
          label: 'Reconnect automatically',
          description: 'Recover on its own after a drop.',
          trailing: FlowToggle(
            value: settings?.autoReconnect ?? true,
            activeColor: c.accent,
            trackColor: c.mat2,
            onChanged: (v) => patch(SettingsPatch(autoReconnect: v)),
          ),
        ),
        SettingRow(
          palette: c,
          label: 'Connect paired devices',
          description: 'Reconnect trusted computers on launch.',
          trailing: FlowToggle(
            value: settings?.autoConnectPairedDevices ?? true,
            activeColor: c.accent,
            trackColor: c.mat2,
            onChanged: (v) => patch(SettingsPatch(autoConnectPairedDevices: v)),
          ),
        ),
      ],
    );
  }
}

class _DeviceLastSeenRow extends ConsumerWidget {
  const _DeviceLastSeenRow({required this.palette, required this.device});

  final FlowPalette palette;
  final Device device;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = palette;
    final canRemove = device.state != DeviceState.active;
    return Container(
      margin: const EdgeInsets.only(bottom: 8),
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      decoration: BoxDecoration(
        color: c.mat1,
        borderRadius: BorderRadius.circular(11),
      ),
      child: Row(
        children: [
          StatusDot(
            color: device.state == DeviceState.active
                ? c.statusActive
                : c.text3,
            filled: device.state == DeviceState.active,
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  device.name,
                  style: FlowType.body(c.text1, weight: FontWeight.w600),
                ),
                Text(
                  '${_osLabel(device.os)} · last seen ${_relativeTime(device.lastSeen)}',
                  style: FlowType.meta(c.text2),
                ),
              ],
            ),
          ),
          if (canRemove)
            GestureDetector(
              onTap: () async {
                try {
                  await ref
                      .read(daemonRepositoryProvider)
                      .removeDevice(device.id);
                  ref.read(toastProvider.notifier).show('Device removed');
                } on DaemonCommandException catch (e) {
                  ref.read(toastProvider.notifier).show(e.message);
                }
              },
              child: Container(
                width: 26,
                height: 26,
                decoration: BoxDecoration(
                  color: c.mat1,
                  borderRadius: BorderRadius.circular(8),
                ),
                alignment: Alignment.center,
                child: Text(
                  '✕',
                  style: TextStyle(fontSize: 11, color: c.text3),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

String _osLabel(HostOs os) => switch (os) {
  HostOs.macos => 'macOS',
  HostOs.windows => 'Windows',
  HostOs.linux => 'Linux',
};

String _relativeTime(DateTime time) {
  final delta = DateTime.now().difference(time);
  if (delta.inSeconds < 60) return 'Now';
  if (delta.inMinutes < 60) return '${delta.inMinutes} min ago';
  if (delta.inHours < 24) return '${delta.inHours} hr ago';
  return '${delta.inDays} day${delta.inDays == 1 ? '' : 's'} ago';
}
