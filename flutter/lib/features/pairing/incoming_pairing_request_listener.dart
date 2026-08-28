import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../domain/daemon_command_exception.dart';
import '../../domain/pairing.dart';
import '../../state/repository_providers.dart';
import 'incoming_pairing_request_dialog.dart';

/// Mounted once, high in the tree. Turns the `incomingPairingRequestProvider`
/// stream into exactly one modal at a time: shows it when a request
/// arrives, pops it when the stream clears (daemon timeout, withdrawal,
/// or another surface answered), and routes the user's choice back to
/// the daemon.
class IncomingPairingRequestListener extends ConsumerStatefulWidget {
  const IncomingPairingRequestListener({
    super.key,
    required this.child,
    this.onShouldSurfaceWindow,
  });

  final Widget child;

  /// The shipped app passes a callback that raises/focuses the OS window;
  /// the dev harness passes `null`.
  final void Function()? onShouldSurfaceWindow;

  @override
  ConsumerState<IncomingPairingRequestListener> createState() =>
      _IncomingPairingRequestListenerState();
}

class _IncomingPairingRequestListenerState
    extends ConsumerState<IncomingPairingRequestListener> {
  /// The request id currently on screen, if a dialog is open.
  String? _shownRequestId;

  @override
  Widget build(BuildContext context) {
    ref.listen(incomingPairingRequestProvider, (previous, next) {
      final request = next.valueOrNull;
      if (request != null && _shownRequestId == null) {
        _show(request);
      } else if (request == null && _shownRequestId != null) {
        _dismiss();
      }
    });
    return widget.child;
  }

  Future<void> _show(IncomingPairingRequest request) async {
    _shownRequestId = request.requestId;
    widget.onShouldSurfaceWindow?.call();

    final decision =
        await showIncomingPairingRequestDialog(context, request) ??
        PairingDecision.reject;

    // Dialog closed by our own _dismiss() (stream already cleared): nothing to send.
    if (_shownRequestId != request.requestId) return;
    _shownRequestId = null;

    try {
      await ref
          .read(daemonRepositoryProvider)
          .respondToPairingRequest(request.requestId, decision);
    } on DaemonCommandException catch (e) {
      if (e.code != 'pairing_request_not_found') rethrow;
    }
  }

  void _dismiss() {
    _shownRequestId = null;
    final navigator = Navigator.of(context, rootNavigator: true);
    if (navigator.canPop()) navigator.pop();
  }
}
