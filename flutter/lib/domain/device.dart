/// The operating system a paired [Device] runs.
enum HostOs { macos, windows, linux }

/// Lifecycle state of a [Device] as seen by the daemon.
///
/// Deliberately identical to `flow_core::device::DeviceState`
/// (`core/src/device/mod.rs`) — same six variants, same meaning — see
/// `docs/contracts/data-model.md`. A real daemon serializes its existing
/// Rust enum straight into this field; there is no translation layer.
enum DeviceState { pairing, connected, active, inactive, disconnected, error }

/// A computer the daemon knows about — paired, or in the process of
/// pairing. See `docs/contracts/data-model.md` for the wire shape.
class Device {
  const Device({
    required this.id,
    required this.name,
    required this.os,
    required this.state,
    required this.lastSeen,
  });

  final String id;
  final String name;
  final HostOs os;
  final DeviceState state;
  final DateTime lastSeen;

  Device copyWith({
    String? id,
    String? name,
    HostOs? os,
    DeviceState? state,
    DateTime? lastSeen,
  }) {
    return Device(
      id: id ?? this.id,
      name: name ?? this.name,
      os: os ?? this.os,
      state: state ?? this.state,
      lastSeen: lastSeen ?? this.lastSeen,
    );
  }

  @override
  bool operator ==(Object other) {
    return other is Device &&
        other.id == id &&
        other.name == name &&
        other.os == os &&
        other.state == state &&
        other.lastSeen == lastSeen;
  }

  @override
  int get hashCode => Object.hash(id, name, os, state, lastSeen);

  @override
  String toString() => 'Device($id, $name, $os, $state)';
}
