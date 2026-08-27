import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/theme/tokens.dart';
import '../../../core/widgets/primitives.dart';
import '../../../domain/daemon_command_exception.dart';
import '../../../domain/daemon_link_state.dart';
import '../../../domain/device.dart';
import '../../../domain/pairing.dart';
import '../../../state/repository_providers.dart';
import '../../../state/ui_providers.dart';
import '../../tray/tray_pairing_view.dart';

const _linkLabels = {
  DaemonLinkState.connected: 'Connected',
  DaemonLinkState.connecting: 'Connecting',
  DaemonLinkState.reconnecting: 'Reconnecting',
  DaemonLinkState.disconnected: 'Disconnected',
  DaemonLinkState.error: 'Error',
  DaemonLinkState.permissionRequired: 'Permission',
};

/// The Dashboard section: the hero "Controlling" card plus the paired
/// devices list with Switch/Remove actions — direct implementation of
/// the `secDashboard` branch.
class DashboardSection extends ConsumerWidget {
  const DashboardSection({super.key, required this.palette});

  final FlowPalette palette;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = palette;
    final devices = ref.watch(devicesProvider).valueOrNull ?? const <Device>[];
    final linkState =
        ref.watch(linkStateProvider).valueOrNull ?? DaemonLinkState.connecting;
    final switchKeyLabel =
        ref.watch(settingsProvider).valueOrNull?.switchKey.label ?? '—';
    final active = devices
        .where((d) => d.state == DeviceState.active)
        .firstOrNull;
    final pairing =
        ref.watch(pairingSessionProvider).valueOrNull ?? PairingSession.idle;

    if (pairing.stage != PairingStage.idle) {
      return TrayPairingView(
        session: pairing,
        palette: c,
        switchKeyLabel: switchKeyLabel,
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Container(
          width: double.infinity,
          margin: const EdgeInsets.only(bottom: 18),
          padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 18),
          decoration: BoxDecoration(
            color: c.accentSoft,
            border: Border.all(color: c.border),
            borderRadius: BorderRadius.circular(15),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text('Controlling', style: FlowType.sectionLabel(c.text3)),
              const SizedBox(height: 7),
              Row(
                children: [
                  StatusDot(color: c.statusActive, size: 11),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(
                      active?.name ?? '—',
                      style: FlowType.heroTitle(c.text1),
                    ),
                  ),
                  Container(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 8,
                      vertical: 4,
                    ),
                    decoration: BoxDecoration(
                      color: c.mat2,
                      borderRadius: BorderRadius.circular(7),
                    ),
                    child: Text(
                      switchKeyLabel,
                      style: FlowType.meta(
                        c.text2,
                      ).copyWith(fontWeight: FontWeight.w700, fontSize: 10.5),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 7),
              Text(_linkLabels[linkState] ?? '', style: FlowType.meta(c.text2)),
            ],
          ),
        ),
        Row(
          mainAxisAlignment: MainAxisAlignment.spaceBetween,
          children: [
            Text(
              'Paired devices',
              style: FlowType.body(
                c.text1,
                weight: FontWeight.w700,
              ).copyWith(fontSize: 15),
            ),
            FlowButton(
              label: 'Pair new device',
              kind: FlowButtonKind.ghost,
              background: c.mat1,
              foreground: c.text1,
              onPressed: () =>
                  ref.read(daemonRepositoryProvider).startPairing(),
            ),
          ],
        ),
        const SizedBox(height: 12),
        for (final device in devices)
          _DashboardDeviceRow(palette: c, device: device),
      ],
    );
  }
}

class _DashboardDeviceRow extends ConsumerWidget {
  const _DashboardDeviceRow({required this.palette, required this.device});

  final FlowPalette palette;
  final Device device;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = palette;
    final canSwitch =
        device.state == DeviceState.inactive ||
        device.state == DeviceState.connected;
    final canRemove = device.state != DeviceState.active;
    final Color dotColor;
    final bool filled;
    switch (device.state) {
      case DeviceState.active:
        dotColor = c.statusActive;
        filled = true;
      case DeviceState.inactive:
      case DeviceState.connected:
        dotColor = c.statusIdle;
        filled = false;
      case DeviceState.error:
        dotColor = c.statusError;
        filled = false;
      default:
        dotColor = c.text3;
        filled = false;
    }
    final metaText = switch (device.state) {
      DeviceState.active => '${_osLabel(device.os)} · Active',
      DeviceState.inactive ||
      DeviceState.connected => '${_osLabel(device.os)} · Connected',
      DeviceState.pairing => '${_osLabel(device.os)} · Pairing…',
      _ => '${_osLabel(device.os)} · Disconnected',
    };

    return Container(
      margin: const EdgeInsets.only(bottom: 8),
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      decoration: BoxDecoration(
        color: c.mat1,
        borderRadius: BorderRadius.circular(11),
      ),
      child: Row(
        children: [
          StatusDot(color: dotColor, filled: filled),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  device.name,
                  style: FlowType.body(
                    device.state == DeviceState.disconnected
                        ? c.text2
                        : c.text1,
                    weight: FontWeight.w600,
                  ),
                ),
                Text(metaText, style: FlowType.meta(c.text2)),
              ],
            ),
          ),
          if (canSwitch)
            FlowButton(
              label: 'Switch',
              kind: FlowButtonKind.primary,
              background: c.accent,
              foreground: c.accentText,
              onPressed: () async {
                try {
                  await ref
                      .read(daemonRepositoryProvider)
                      .switchActiveDevice(device.id);
                  ref.read(toastProvider.notifier).show('Switched');
                } on DaemonCommandException catch (e) {
                  ref.read(toastProvider.notifier).show(e.message);
                }
              },
            ),
          if (canRemove) ...[
            const SizedBox(width: 6),
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

extension<T> on Iterable<T> {
  T? get firstOrNull => isEmpty ? null : first;
}
