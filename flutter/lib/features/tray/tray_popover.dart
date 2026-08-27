import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/platform_chrome.dart';
import '../../core/theme/flow_theme.dart';
import '../../core/theme/tokens.dart';
import '../../core/widgets/glass_surface.dart';
import '../../core/widgets/primitives.dart';
import '../../domain/daemon_command_exception.dart';
import '../../domain/daemon_link_state.dart';
import '../../domain/device.dart';
import '../../domain/pairing.dart';
import '../../state/repository_providers.dart';
import '../../state/ui_providers.dart';
import 'tray_pairing_view.dart';

/// The tray/menu-bar popover — direct implementation of `TrayPopover.
/// dc.html`. 312px wide, radius/edge behavior keyed off [platform]. Swaps
/// to [TrayPairingView] whenever a pairing session is active
/// (`docs/contracts/daemon-ipc.md`'s pairing state machine), otherwise
/// shows the main menu: status header, an optional link-health banner,
/// the active device, other paired devices, and the Pair/Dashboard/
/// Settings/Quit menu rows.
class TrayPopover extends ConsumerStatefulWidget {
  const TrayPopover({
    super.key,
    required this.platform,
    this.onOpenDashboard,
    this.onOpenSettings,
  });

  final HostOs platform;
  final VoidCallback? onOpenDashboard;
  final VoidCallback? onOpenSettings;

  @override
  ConsumerState<TrayPopover> createState() => _TrayPopoverState();
}

class _TrayPopoverState extends ConsumerState<TrayPopover> {
  String? _switchingDeviceId;

  Future<void> _switchTo(String deviceId) async {
    setState(() => _switchingDeviceId = deviceId);
    try {
      await ref.read(daemonRepositoryProvider).switchActiveDevice(deviceId);
      ref.read(toastProvider.notifier).show('Switched');
    } on DaemonCommandException catch (e) {
      ref.read(toastProvider.notifier).show(e.message);
    } finally {
      if (mounted) setState(() => _switchingDeviceId = null);
    }
  }

  Future<void> _recover(DaemonLinkState state) async {
    final repo = ref.read(daemonRepositoryProvider);
    switch (state) {
      case DaemonLinkState.reconnecting:
        // "Cancel" — the mock has no direct reconnect-cancel command;
        // this is a UI-only affordance until the contract grows one.
        break;
      case DaemonLinkState.permissionRequired:
        await repo.requestPermission();
        break;
      case DaemonLinkState.disconnected:
      case DaemonLinkState.error:
        // Actually asks the daemon to try again — watchLinkState is what
        // reports whether it worked, never a toast asserting success
        // before the daemon has done anything.
        try {
          await repo.retryConnection();
        } on DaemonCommandException catch (e) {
          ref.read(toastProvider.notifier).show(e.message);
        }
        break;
      case DaemonLinkState.connected:
      case DaemonLinkState.connecting:
        break;
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = FlowColors.of(context);
    final chrome = PlatformChrome.of(widget.platform);

    final devices = ref.watch(devicesProvider).valueOrNull ?? const <Device>[];
    final linkState =
        ref.watch(linkStateProvider).valueOrNull ?? DaemonLinkState.connecting;
    final pairing =
        ref.watch(pairingSessionProvider).valueOrNull ?? PairingSession.idle;
    final switchKeyLabel =
        ref.watch(settingsProvider).valueOrNull?.switchKey.label ?? '—';

    return GlassSurface(
      color: c.trayGlass,
      border: c.border,
      blurSigma: 44,
      insetHighlight: true,
      borderRadius: BorderRadius.circular(chrome.popoverRadius()),
      boxShadow: c.shadow,
      child: SizedBox(
        width: 312,
        child: pairing.stage == PairingStage.idle
            ? _MainMenu(
                palette: c,
                devices: devices,
                linkState: linkState,
                switchKeyLabel: switchKeyLabel,
                switchingDeviceId: _switchingDeviceId,
                onSwitch: _switchTo,
                onRecover: () => _recover(linkState),
                onStartPairing: () =>
                    ref.read(daemonRepositoryProvider).startPairing(),
                onOpenDashboard: widget.onOpenDashboard,
                onOpenSettings: widget.onOpenSettings,
              )
            : TrayPairingView(
                session: pairing,
                palette: c,
                switchKeyLabel: switchKeyLabel,
              ),
      ),
    );
  }
}

class _LinkMeta {
  const _LinkMeta(this.label, this.dot, this.pulse, this.banner, this.action);

  final String label;
  final Color dot;
  final bool pulse;
  final String? banner;
  final String? action;
}

_LinkMeta _linkMeta(DaemonLinkState state, FlowPalette c) {
  return switch (state) {
    DaemonLinkState.connected => _LinkMeta(
      'Connected',
      c.statusActive,
      false,
      null,
      null,
    ),
    DaemonLinkState.connecting => _LinkMeta(
      'Connecting…',
      c.statusPending,
      true,
      null,
      null,
    ),
    DaemonLinkState.reconnecting => _LinkMeta(
      'Reconnecting…',
      c.statusPending,
      true,
      'Work Laptop dropped out. Trying again.',
      'Cancel',
    ),
    DaemonLinkState.disconnected => _LinkMeta(
      'Disconnected',
      c.statusOffline,
      false,
      'Work Laptop is unavailable.',
      'Retry',
    ),
    DaemonLinkState.error => _LinkMeta(
      'Connection lost',
      c.statusError,
      false,
      'Input sharing paused until Work Laptop is back.',
      'Retry',
    ),
    DaemonLinkState.permissionRequired => _LinkMeta(
      'Needs permission',
      c.statusError,
      false,
      'Allow input access to share your keyboard.',
      'Allow',
    ),
  };
}

class _MainMenu extends StatelessWidget {
  const _MainMenu({
    required this.palette,
    required this.devices,
    required this.linkState,
    required this.switchKeyLabel,
    required this.switchingDeviceId,
    required this.onSwitch,
    required this.onRecover,
    required this.onStartPairing,
    required this.onOpenDashboard,
    required this.onOpenSettings,
  });

  final FlowPalette palette;
  final List<Device> devices;
  final DaemonLinkState linkState;
  final String switchKeyLabel;
  final String? switchingDeviceId;
  final ValueChanged<String> onSwitch;
  final VoidCallback onRecover;
  final VoidCallback onStartPairing;
  final VoidCallback? onOpenDashboard;
  final VoidCallback? onOpenSettings;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    final meta = _linkMeta(linkState, c);
    final active =
        devices.where((d) => d.state == DeviceState.active).firstOrNull ??
        devices.firstOrNull;
    final others = devices.where((d) => d.id != active?.id).toList();

    return Padding(
      padding: const EdgeInsets.only(bottom: 9),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 13, 14, 11),
            child: Row(
              children: [
                Container(
                  width: 26,
                  height: 26,
                  decoration: BoxDecoration(
                    color: c.accent,
                    borderRadius: BorderRadius.circular(8),
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(
                        'Cross Device',
                        style: FlowType.body(
                          c.text1,
                          weight: FontWeight.w700,
                        ).copyWith(fontSize: 13.5),
                      ),
                      Text(meta.label, style: FlowType.meta(c.text2)),
                    ],
                  ),
                ),
                StatusDot(color: meta.dot, pulse: meta.pulse),
              ],
            ),
          ),
          if (meta.banner != null)
            Container(
              margin: const EdgeInsets.fromLTRB(12, 0, 12, 10),
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
              decoration: BoxDecoration(
                color: c.mat1,
                borderRadius: BorderRadius.circular(11),
              ),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      meta.banner!,
                      style: FlowType.meta(c.text2).copyWith(height: 1.35),
                    ),
                  ),
                  const SizedBox(width: 10),
                  FlowButton(
                    label: meta.action!,
                    kind: FlowButtonKind.ghost,
                    background: c.mat2,
                    foreground: c.text1,
                    onPressed: onRecover,
                  ),
                ],
              ),
            ),
          _SectionLabel('Using', palette: c),
          if (active != null)
            Container(
              margin: const EdgeInsets.fromLTRB(8, 0, 8, 8),
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 11),
              decoration: BoxDecoration(
                color: c.accentSoft,
                border: Border.all(color: c.border),
                borderRadius: BorderRadius.circular(10),
              ),
              child: Row(
                children: [
                  StatusDot(color: c.statusActive),
                  const SizedBox(width: 11),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          active.name,
                          style: FlowType.body(
                            c.text1,
                            weight: FontWeight.w700,
                          ).copyWith(fontSize: 14),
                        ),
                        Text(
                          'Active · ${_osLabel(active.os)}',
                          style: FlowType.meta(c.text2),
                        ),
                      ],
                    ),
                  ),
                  Container(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 7,
                      vertical: 3,
                    ),
                    decoration: BoxDecoration(
                      color: c.mat2,
                      borderRadius: BorderRadius.circular(6),
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
            ),
          if (others.isNotEmpty) _SectionLabel('Devices', palette: c),
          for (final device in others)
            _DeviceRow(
              device: device,
              palette: c,
              switching: switchingDeviceId == device.id,
              linkConnecting: linkState == DaemonLinkState.connecting,
              onTap:
                  device.state == DeviceState.inactive ||
                      device.state == DeviceState.connected
                  ? () => onSwitch(device.id)
                  : null,
            ),
          _MenuRow(
            palette: c,
            leading: Text(
              '+',
              style: TextStyle(
                fontSize: 14,
                fontWeight: FontWeight.w700,
                color: c.accent,
              ),
            ),
            label: 'Pair New Device',
            onTap: onStartPairing,
          ),
          Container(
            margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
            height: 1,
            color: c.hairline,
          ),
          _MenuRow(
            palette: c,
            label: 'Dashboard',
            trailing: '↗',
            onTap: onOpenDashboard,
          ),
          _MenuRow(
            palette: c,
            label: 'Settings',
            trailing: '↗',
            onTap: onOpenSettings,
          ),
          _MenuRow(palette: c, label: 'Quit Cross Device', muted: true),
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

String _deviceStateText(
  DeviceState state, {
  required bool switching,
  required bool linkConnecting,
}) {
  if (switching) return 'Switching…';
  return switch (state) {
    DeviceState.inactive ||
    DeviceState.connected => linkConnecting ? 'Connecting…' : 'Connected',
    DeviceState.disconnected => 'Disconnected',
    DeviceState.pairing => 'Pairing…',
    DeviceState.error => 'Error',
    DeviceState.active => 'Active',
  };
}

class _SectionLabel extends StatelessWidget {
  const _SectionLabel(this.label, {required this.palette});

  final String label;
  final FlowPalette palette;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 4, 20, 6),
      child: Text(label, style: FlowType.sectionLabel(palette.text3)),
    );
  }
}

class _DeviceRow extends StatelessWidget {
  const _DeviceRow({
    required this.device,
    required this.palette,
    required this.switching,
    required this.linkConnecting,
    this.onTap,
  });

  final Device device;
  final FlowPalette palette;
  final bool switching;
  final bool linkConnecting;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    final switchable =
        device.state == DeviceState.inactive ||
        device.state == DeviceState.connected;
    final Color dotColor;
    final bool filled;
    if (switching) {
      dotColor = c.statusPending;
      filled = true;
    } else if (switchable) {
      dotColor = linkConnecting ? c.statusPending : c.statusIdle;
      filled = false;
    } else if (device.state == DeviceState.error) {
      dotColor = c.statusError;
      filled = false;
    } else {
      dotColor = c.text3;
      filled = false;
    }

    return GestureDetector(
      onTap: onTap,
      child: Container(
        margin: const EdgeInsets.fromLTRB(8, 0, 8, 3),
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
        decoration: BoxDecoration(
          color: switching ? c.mat2 : c.mat1,
          borderRadius: BorderRadius.circular(10),
        ),
        child: Row(
          children: [
            StatusDot(
              color: dotColor,
              filled: filled,
              pulse: switching || (switchable && linkConnecting),
            ),
            const SizedBox(width: 11),
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
                  Text(
                    _deviceStateText(
                      device.state,
                      switching: switching,
                      linkConnecting: linkConnecting,
                    ),
                    style: FlowType.meta(c.text2),
                  ),
                ],
              ),
            ),
            if (onTap != null)
              Text('→', style: FlowType.body(c.text3).copyWith(fontSize: 13)),
          ],
        ),
      ),
    );
  }
}

class _MenuRow extends StatelessWidget {
  const _MenuRow({
    required this.palette,
    required this.label,
    this.leading,
    this.trailing,
    this.onTap,
    this.muted = false,
  });

  final FlowPalette palette;
  final String label;
  final Widget? leading;
  final String? trailing;
  final VoidCallback? onTap;
  final bool muted;

  @override
  Widget build(BuildContext context) {
    final c = palette;
    return GestureDetector(
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        child: Row(
          children: [
            if (leading != null) ...[
              SizedBox(width: 9, child: Center(child: leading)),
              const SizedBox(width: 11),
            ],
            Expanded(
              child: Text(
                label,
                style: FlowType.body(
                  muted ? c.text2 : c.text1,
                  weight: FontWeight.w500,
                ),
              ),
            ),
            if (trailing != null)
              Text(trailing!, style: FlowType.meta(c.text3)),
          ],
        ),
      ),
    );
  }
}

extension<T> on Iterable<T> {
  T? get firstOrNull => isEmpty ? null : first;
}
