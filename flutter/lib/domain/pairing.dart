import 'device.dart';

/// Stage of a pairing attempt. See `docs/contracts/daemon-ipc.md` for the
/// full state machine: idle -> searching -> found -> requesting ->
/// (paired | failed) -> idle.
enum PairingStage { idle, searching, found, requesting, paired, failed }

/// A nearby device discovered while searching, not yet paired.
class PairingCandidate {
  const PairingCandidate({required this.id, required this.name, required this.os});

  final String id;
  final String name;
  final HostOs os;

  @override
  bool operator ==(Object other) {
    return other is PairingCandidate && other.id == id && other.name == name && other.os == os;
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
