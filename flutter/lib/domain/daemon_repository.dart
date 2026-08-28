import 'daemon_link_state.dart';
import 'device.dart';
import 'pairing.dart';
import 'permission_status.dart';
import 'settings.dart';
import 'switch_key_binding.dart';

/// The full contract between the Flutter UI and the daemon, documented in
/// `docs/contracts/daemon-ipc.md`. This is the only way anything in `lib/`
/// may read daemon state or ask the daemon to do something — no widget or
/// provider may depend on [MockDaemonRepository] (or, later, a real IPC
/// client) directly, only on this interface, so either can be swapped in
/// without touching UI code.
///
/// State flows one way, out through the `watch*` streams — never through a
/// command's return value — because device state, link health, and
/// settings can all change from something happening on another computer,
/// not just from a local command; a stream is the only shape that stays
/// correct in that case. Each stream replays its current value to a new
/// listener, so subscribing *is* the initial fetch — there is no separate
/// `getDevices()`-style method.
///
/// Commands return `Future<void>` that resolves once the daemon has
/// accepted the command (or throws if it rejected it) — the effect itself
/// always shows up on a stream, never in the future's value.
abstract class DaemonRepository {
  Stream<List<Device>> watchDevices();
  Stream<DaemonLinkState> watchLinkState();
  Stream<PairingSession> watchPairingSession();
  Stream<FlowSettings> watchSettings();
  Stream<PermissionStatus> watchPermission();

  /// The pairing request another device has sent *to* this machine and that
  /// is awaiting the local user's Accept/Reject, or `null` when nothing is
  /// pending. Only one can be outstanding at a time.
  Stream<IncomingPairingRequest?> watchIncomingPairingRequest();

  /// Answers the outstanding [IncomingPairingRequest]. [requestId] must
  /// match the currently pending request, otherwise this throws with code
  /// `pairing_request_not_found`. On [PairingDecision.accept] the peer
  /// shows up on [watchDevices]; either way [watchIncomingPairingRequest]
  /// goes back to `null`.
  Future<void> respondToPairingRequest(
    String requestId,
    PairingDecision decision,
  );

  /// Moves [deviceId] to [DeviceState.active] and demotes the previous
  /// active device to [DeviceState.inactive]. Requires the target device
  /// to currently be [DeviceState.inactive] or [DeviceState.connected].
  Future<void> switchActiveDevice(String deviceId);

  /// Drops a paired device. The local device ("This device") can never be
  /// removed.
  Future<void> removeDevice(String deviceId);

  /// Starts discovery. Requires [PairingSession.stage] to be
  /// [PairingStage.idle].
  Future<void> startPairing();

  /// Abandons an in-progress pairing attempt, resetting the session to
  /// [PairingStage.idle].
  Future<void> cancelPairing();

  /// Requests pairing with a discovered candidate. Requires
  /// [PairingSession.stage] to be [PairingStage.found] and [candidateId]
  /// to be one of the current [PairingSession.candidates].
  Future<void> pairWithCandidate(String candidateId);

  /// Sets the active switch-key shortcut. [binding] must be non-empty.
  Future<void> setSwitchKey(SwitchKeyBinding binding);

  /// Merges [patch] into the current settings.
  Future<void> updateSettings(SettingsPatch patch);

  /// Restores settings to [FlowSettings.defaults].
  Future<void> resetSettings();

  /// Triggers the OS permission prompt. Requires the current
  /// [PermissionStatus.granted] to be `false`.
  Future<void> requestPermission();

  /// Reissues a connection attempt after the link has been given up on —
  /// `docs/contracts/daemon-ipc.md`'s `disconnected --(user retries)-->
  /// connecting` and `error --(user retries)--> connecting` transitions.
  /// Requires the current [DaemonLinkState] to be [DaemonLinkState.disconnected]
  /// or [DaemonLinkState.error]. Only moves the state to `connecting`, not
  /// straight to `connected` — actual recovery still shows up on
  /// [watchLinkState] once (if) it happens, same as any other command.
  Future<void> retryConnection();
}
