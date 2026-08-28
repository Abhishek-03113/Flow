import 'device.dart';

/// Stage of a pairing attempt. See `docs/contracts/daemon-ipc.md` for the
/// full state machine: idle -> searching -> found -> requesting ->
/// (paired | failed) -> idle.
enum PairingStage { idle, searching, found, requesting, paired, failed }

/// A nearby device discovered while searching, not yet paired.
class PairingCandidate {
  const PairingCandidate({
    required this.id,
    required this.name,
    required this.os,
  });

  final String id;
  final String name;
  final HostOs os;

  @override
  bool operator ==(Object other) {
    return other is PairingCandidate &&
        other.id == id &&
        other.name == name &&
        other.os == os;
  }

  @override
  int get hashCode => Object.hash(id, name, os);
}

/// Current state of the (single, app-wide) pairing flow.
class PairingSession {
  const PairingSession({
    this.stage = PairingStage.idle,
    this.candidates = const [],
    this.targetName,
    this.error,
  });

  final PairingStage stage;

  /// Populated once [stage] is [PairingStage.found] or later.
  final List<PairingCandidate> candidates;

  /// Set once [stage] is [PairingStage.requesting] or later.
  final String? targetName;

  /// Set only when [stage] is [PairingStage.failed].
  final String? error;

  PairingSession copyWith({
    PairingStage? stage,
    List<PairingCandidate>? candidates,
    String? targetName,
    String? error,
  }) {
    return PairingSession(
      stage: stage ?? this.stage,
      candidates: candidates ?? this.candidates,
      targetName: targetName ?? this.targetName,
      error: error ?? this.error,
    );
  }

  static const idle = PairingSession();
}

/// The local user's answer to an incoming pairing request.
enum PairingDecision {
  accept,
  reject;

  String get wireName => switch (this) {
    PairingDecision.accept => 'accept',
    PairingDecision.reject => 'reject',
  };
}

/// An incoming pairing request awaiting this user's Accept/Reject.
/// Mirrors `docs/contracts/data-model.md`'s `IncomingPairingRequest`.
class IncomingPairingRequest {
  const IncomingPairingRequest({
    required this.requestId,
    required this.deviceName,
    required this.deviceOs,
    required this.fingerprint,
    required this.address,
  });

  final String requestId;
  final String deviceName;
  final HostOs deviceOs;
  final String fingerprint;
  final String address;

  @override
  bool operator ==(Object other) =>
      other is IncomingPairingRequest &&
      other.requestId == requestId &&
      other.deviceName == deviceName &&
      other.deviceOs == deviceOs &&
      other.fingerprint == fingerprint &&
      other.address == address;

  @override
  int get hashCode =>
      Object.hash(requestId, deviceName, deviceOs, fingerprint, address);
}

/// `null` in ⇒ `null` out (the stream carries `null` when nothing is pending).
IncomingPairingRequest? incomingPairingRequestFromJson(Object? json) {
  if (json == null) return null;
  final map = json as Map<String, dynamic>;
  return IncomingPairingRequest(
    requestId: map['request_id'] as String,
    deviceName: map['device_name'] as String,
    deviceOs: switch (map['device_os'] as String) {
      'macos' => HostOs.macos,
      'windows' => HostOs.windows,
      _ => HostOs.linux,
    },
    fingerprint: map['fingerprint'] as String,
    address: map['address'] as String? ?? '',
  );
}
