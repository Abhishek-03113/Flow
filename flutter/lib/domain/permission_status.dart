/// The daemon's OS input-capture permission (Accessibility on macOS,
/// input access on Windows, input device access on Linux). Surfaced
/// during onboarding and in Advanced settings.
///
/// [name] is daemon-supplied (it already knows which platform it's
/// running on) rather than derived client-side, so the UI never
/// hardcodes per-OS permission copy.
class PermissionStatus {
  const PermissionStatus({required this.name, required this.granted});

  final String name;
  final bool granted;

  PermissionStatus copyWith({String? name, bool? granted}) {
    return PermissionStatus(
      name: name ?? this.name,
      granted: granted ?? this.granted,
    );
  }
}
