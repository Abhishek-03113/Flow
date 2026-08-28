@Tags(['e2e'])
@Timeout(Duration(minutes: 5))
library;

import 'dart:async';
import 'dart:io';

import 'package:flow_ui/core/theme/flow_theme.dart';
import 'package:flow_ui/data/ipc_constants.dart';
import 'package:flow_ui/data/ipc_daemon_repository.dart';
import 'package:flow_ui/domain/device.dart';
import 'package:flow_ui/domain/pairing.dart';
import 'package:flow_ui/domain/settings.dart';
import 'package:flow_ui/features/app_window/app_window_shell.dart';
import 'package:flow_ui/features/onboarding/onboarding_flow.dart';
import 'package:flow_ui/features/tray/tray_popover.dart';
import 'package:flow_ui/state/repository_providers.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:web_socket_channel/web_socket_channel.dart';

/// True end-to-end coverage of the flow this project is built around:
/// a real `flow-daemon` process, talked to over the real local IPC
/// WebSocket contract (`docs/contracts/daemon-ipc.md`), driving the real
/// production widgets ([TrayPopover], [AppWindowShell], [OnboardingFlow])
/// through actual taps — not the mock, and not just the repository layer.
///
/// This automates `docs/testing/manual-testing-strategy.md`'s Tier 0
/// checklist ("Flutter ↔ real daemon, one machine") plus the restart/
/// kill scenarios from Tiers 0 and 3, as a single `flutter test` run
/// instead of a human following steps across two terminals. It picks up
/// where `test/data/ipc_daemon_repository_manual_test.dart` leaves off:
/// that file proves the *repository* layer matches the mock's contract;
/// this file proves the same real daemon drives the *actual screens* a
/// user sees, end to end.
///
/// ## Running it
///
/// Requires `cargo` on `PATH` (it builds `flow-daemon` itself — no
/// manually-started process, no second terminal) and nothing else
/// already bound to `127.0.0.1:47823`:
///
/// ```sh
/// cd flutter
/// flutter test --tags e2e --run-skipped test/e2e/daemon_ui_flow_e2e_test.dart
/// ```
///
/// Tagged `e2e` (`dart_test.yaml` skips it by default, same as `manual`)
/// since it needs a Rust toolchain, real loopback networking, and takes
/// on the order of 15-20 seconds — never part of a plain `flutter test`.
///
/// All tests in this file share one running `flow-daemon` process (a
/// fixed IPC port means only one instance can be up at a time) and run
/// in the declared order — later tests assume the state earlier ones
/// left behind (e.g. the "restart persists state" test assumes the
/// active-device switch and device removal from earlier tests already
/// happened), exactly like the manual contract test's own sharing model.
void main() {
  late Directory homeDir;
  late Directory repoRoot;
  late Process daemonProcess;
  late IpcDaemonRepository repo;
  var daemonAlreadyStopped = false;

  // setUpAll can fail partway (e.g. the port-free check, or the cargo
  // build) before homeDir/daemonProcess/repo above are ever assigned —
  // tearDownAll still runs in that case, so it must not assume any of
  // them exist.
  var homeDirCreated = false;
  var daemonStarted = false;
  var repoConnected = false;

  // Set once the onboarding test successfully pairs a new device, so the
  // restart-persistence test can confirm it survived, without hardcoding
  // which of the two candidate-pool entries the daemon happened to offer.
  String? pairedDeviceName;

  // `testWidgets` bodies run inside flutter_test's `FakeAsync` zone, which
  // intercepts real `Timer`s (including `.timeout()` and `Future.delayed`)
  // so they only fire when a `pump()` call elapses the fake clock. A bare
  // `await` on this suite's real socket/process work — none of it
  // fake-clocked — would silently never resolve. Every such wait below
  // goes through `tester.runAsync`, which steps outside the fake zone for
  // real async work to actually complete; `settle` (below) is what then
  // lets the already-arrived result propagate into the (fake-zoned)
  // widget tree via ordinary pumps.
  Future<void> settle(WidgetTester tester) async {
    // Deliberately not `pumpAndSettle`: `FlowSpinner`/pulsing status dots
    // animate indefinitely while searching/connecting, which would make
    // `pumpAndSettle` time out. A bounded number of real pumps is enough
    // to flush an already-completed `runAsync` result into a rebuild.
    for (var i = 0; i < 20; i++) {
      await tester.pump(const Duration(milliseconds: 50));
    }
  }

  Widget harness(Widget child) {
    return ProviderScope(
      overrides: [daemonRepositoryProvider.overrideWithValue(repo)],
      child: MaterialApp(
        debugShowCheckedModeBanner: false,
        theme: FlowTheme.light(),
        home: Scaffold(body: Center(child: child)),
      ),
    );
  }

  Future<void> waitForPortOpen(
    int port, {
    Duration timeout = const Duration(seconds: 20),
  }) async {
    final deadline = DateTime.now().add(timeout);
    while (DateTime.now().isBefore(deadline)) {
      try {
        final socket = await Socket.connect(
          '127.0.0.1',
          port,
          timeout: const Duration(milliseconds: 500),
        );
        await socket.close();
        return;
      } catch (_) {
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
    }
    throw StateError('flow-daemon never opened port $port within $timeout');
  }

  Future<void> ensurePortFree(int port) async {
    try {
      final socket = await Socket.connect(
        '127.0.0.1',
        port,
        timeout: const Duration(milliseconds: 300),
      );
      await socket.close();
    } catch (_) {
      return; // nothing listening yet — expected.
    }
    fail(
      'port $port is already in use by another process — stop whatever is '
      'bound to it before running this suite (only one flow-daemon can '
      'bind 127.0.0.1:$port at a time)',
    );
  }

  Future<String> waitForToken(
    String home, {
    Duration timeout = const Duration(seconds: 20),
  }) async {
    final file = File('$home/.flow/ipc.token');
    final deadline = DateTime.now().add(timeout);
    while (DateTime.now().isBefore(deadline)) {
      if (file.existsSync()) {
        final token = file.readAsStringSync().trim();
        if (token.isNotEmpty) return token;
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    throw StateError('flow-daemon never wrote its IPC token to ${file.path}');
  }

  String daemonBinaryPath() {
    final name = Platform.isWindows ? 'flow-daemon.exe' : 'flow-daemon';
    return '${repoRoot.path}/target/debug/$name';
  }

  Future<Process> startDaemon() async {
    final process = await Process.start(
      daemonBinaryPath(),
      const [],
      environment: {
        ...Platform.environment,
        'HOME': homeDir.path,
        'USERPROFILE': homeDir.path,
        // Pin the daemon's database and token file into the scratch dir
        // explicitly, rather than relying on HOME/USERPROFILE redirection
        // alone: on Windows the `directories` crate resolves the platform
        // data dir from the OS, not the environment, so a redirected
        // home doesn't move the database there. These are the same
        // env overrides `daemon/README.md` documents for running a
        // second instance.
        'FLOW_DATA_DIR': '${homeDir.path}/data',
        'FLOW_IPC_TOKEN_PATH': '${homeDir.path}/.flow/ipc.token',
        'XDG_DATA_HOME': '${homeDir.path}/.local/share',
        'XDG_CONFIG_HOME': '${homeDir.path}/.config',
        'XDG_CACHE_HOME': '${homeDir.path}/.cache',
        'RUST_LOG': 'info',
        // This suite's own assertions are written against the
        // mock-parity fixture (MacBook/Work Laptop/Desktop, non-empty
        // pairing candidates) — a real flow-daemon process never seeds
        // that by default (`daemon/README.md` "Removing mock runtime
        // data"), so this is the explicit opt-in a test needs.
        'FLOW_DAEMON_SEED_MOCK_PARITY': '1',
      },
      workingDirectory: repoRoot.path,
    );
    process.stdout
        .transform(const SystemEncoding().decoder)
        .listen((line) => stdout.writeln('[flow-daemon] $line'));
    process.stderr
        .transform(const SystemEncoding().decoder)
        .listen((line) => stderr.writeln('[flow-daemon] $line'));
    return process;
  }

  Future<IpcDaemonRepository> connect() async {
    final token = await waitForToken(homeDir.path);
    final repository = IpcDaemonRepository.withChannel(
      WebSocketChannel.connect(flowDaemonIpcUri(), protocols: [token]),
    );
    // A brand new repository's ReplayChannels start empty; wait for the
    // daemon's initial state frames so a caller that immediately pumps a
    // widget tree isn't racing the socket — `devicesProvider` et al.
    // would otherwise sit in `AsyncLoading` (rendering nothing) until
    // those frames happen to arrive.
    await repository.watchDevices().first;
    await repository.watchSettings().first;
    return repository;
  }

  /// Kills the current `flow-daemon` process, starts a fresh one against
  /// the *same* data directory, and reconnects — the real-process version
  /// of the manual checklist's "kill and restart flow-daemon; confirm ...
  /// persists" (SQLite, not process memory). Per
  /// `daemon/src/storage/device_repo.rs`'s documented contract, a
  /// device's *identity* (id/name/os/pairing) persists but its
  /// connection `DeviceState` deliberately does not — every device
  /// reloads as `Disconnected` until a live connection re-establishes
  /// it, "never resurrected as Active from a stale row". The
  /// persistence test below asserts against that real contract, not the
  /// pre-restart `DeviceState`.
  Future<void> restartDaemon() async {
    await repo.dispose();
    daemonProcess.kill(ProcessSignal.sigterm);
    await daemonProcess.exitCode.timeout(
      const Duration(seconds: 10),
      onTimeout: () {
        daemonProcess.kill(ProcessSignal.sigkill);
        return -1;
      },
    );
    daemonProcess = await startDaemon();
    await waitForPortOpen(kFlowDaemonIpcPort);
    repo = await connect();
  }

  setUpAll(() async {
    repoRoot = Directory.current.path.endsWith('flutter')
        ? Directory.current.parent
        : Directory.current;
    if (!File('${repoRoot.path}/Cargo.toml').existsSync()) {
      fail(
        'could not find the workspace Cargo.toml next to ${repoRoot.path} — '
        'run this suite from the flutter/ package root (`cd flutter && '
        'flutter test --tags e2e --run-skipped test/e2e/daemon_ui_flow_e2e_test.dart`)',
      );
    }

    await ensurePortFree(kFlowDaemonIpcPort);

    final build = await Process.run('cargo', [
      'build',
      '-p',
      'flow-daemon',
    ], workingDirectory: repoRoot.path);
    if (build.exitCode != 0) {
      fail(
        'cargo build -p flow-daemon failed:\n${build.stdout}\n${build.stderr}',
      );
    }

    homeDir = await Directory.systemTemp.createTemp('flow-e2e-home-');
    homeDirCreated = true;
    daemonProcess = await startDaemon();
    daemonStarted = true;
    await waitForPortOpen(kFlowDaemonIpcPort);
    repo = await connect();
    repoConnected = true;
  });

  tearDownAll(() async {
    if (repoConnected) {
      try {
        await repo.dispose();
      } catch (_) {
        // already torn down by the kill-mid-session test — fine.
      }
    }
    if (daemonStarted && !daemonAlreadyStopped) {
      daemonProcess.kill(ProcessSignal.sigkill);
      await daemonProcess.exitCode.timeout(
        const Duration(seconds: 10),
        onTimeout: () => -1,
      );
    }
    if (homeDirCreated) {
      await homeDir.delete(recursive: true);
    }
  });

  testWidgets(
    'tray popover reflects the real daemon\'s seeded devices and link '
    'state over IPC',
    (tester) async {
      await tester.pumpWidget(
        harness(const TrayPopover(platform: HostOs.macos)),
      );
      await settle(tester);

      expect(find.text('MacBook'), findsOneWidget);
      expect(find.text('Active · macOS'), findsOneWidget);
      expect(find.text('Work Laptop'), findsOneWidget);
      expect(find.text('Desktop'), findsOneWidget);
      expect(find.text('Connected'), findsWidgets);
    },
  );

  testWidgets(
    'switching the active device from the tray popover commands the real '
    'daemon and the change round-trips back into the UI',
    (tester) async {
      await tester.pumpWidget(
        harness(const TrayPopover(platform: HostOs.macos)),
      );
      await settle(tester);

      await tester.tap(find.text('Work Laptop'));
      late List<Device> devices;
      await tester.runAsync(() async {
        devices = await repo
            .watchDevices()
            .firstWhere(
              (list) => list.any(
                (d) => d.id == 'd2' && d.state == DeviceState.active,
              ),
            )
            .timeout(const Duration(seconds: 5));
      });
      expect(
        devices.singleWhere((d) => d.id == 'd1').state,
        DeviceState.inactive,
      );

      await settle(tester);
      expect(find.text('Active · Windows'), findsOneWidget);
    },
  );

  testWidgets('the Input settings section changes the switch key on the real '
      'daemon and the change round-trips back into the UI', (tester) async {
    await tester.pumpWidget(
      harness(
        const AppWindowShell(
          platform: HostOs.linux,
          initialSection: AppSection.input,
        ),
      ),
    );
    await settle(tester);
    expect(find.text('Scroll Lock'), findsWidgets); // default: header + chip

    await tester.tap(find.text('F13'));
    late FlowSettings settings;
    await tester.runAsync(() async {
      settings = await repo
          .watchSettings()
          .firstWhere((s) => s.switchKey.label == 'F13')
          .timeout(const Duration(seconds: 5));
    });
    expect(settings.switchKey.label, 'F13');

    await settle(tester);
    expect(find.text('F13'), findsNWidgets(2)); // header value + selected chip
  });

  testWidgets(
    'the Devices settings section removes a device on the real daemon '
    'and it disappears from the UI',
    (tester) async {
      await tester.pumpWidget(
        harness(
          const AppWindowShell(
            platform: HostOs.linux,
            initialSection: AppSection.devices,
          ),
        ),
      );
      await settle(tester);
      expect(find.text('Desktop'), findsOneWidget);

      final row = find
          .ancestor(of: find.text('Desktop'), matching: find.byType(Container))
          .first;
      await tester.tap(find.descendant(of: row, matching: find.text('✕')));

      await tester.runAsync(() async {
        await repo
            .watchDevices()
            .firstWhere((list) => !list.any((d) => d.id == 'd3'))
            .timeout(const Duration(seconds: 5));
      });

      await settle(tester);
      expect(find.text('Desktop'), findsNothing);
    },
  );

  testWidgets(
    'the onboarding flow completes a full pairing handshake against the '
    'real daemon, widget by widget',
    (tester) async {
      var onboardingDone = false;
      await tester.pumpWidget(
        harness(
          OnboardingFlow(
            platform: HostOs.macos,
            onDone: () => onboardingDone = true,
          ),
        ),
      );
      await settle(tester);

      // Step 0: welcome.
      expect(find.text('Continue'), findsOneWidget);
      await tester.tap(find.text('Continue'));
      await settle(tester);

      // Step 1: permission — grant it against the real daemon.
      expect(find.text('Allow'), findsOneWidget);
      await tester.tap(find.text('Allow'));
      await tester.runAsync(() async {
        await repo
            .watchPermission()
            .firstWhere((p) => p.granted)
            .timeout(const Duration(seconds: 5));
      });
      await settle(tester);
      expect(find.text('Granted'), findsWidgets);

      await tester.tap(find.text('Continue'));
      await settle(tester);

      // Step 2: pairing — entering this step auto-calls startPairing()
      // (onboarding_flow.dart _goTo), so just wait for the real daemon's
      // search-to-found transition, same timers `daemon/todos.json`'s
      // mock-parity fallback uses when nothing has been discovered live.
      late PairingSession found;
      await tester.runAsync(() async {
        found = await repo
            .watchPairingSession()
            .firstWhere((s) => s.stage == PairingStage.found)
            .timeout(const Duration(seconds: 5));
      });
      expect(found.candidates, isNotEmpty);
      final candidateName = found.candidates.first.name;

      await settle(tester);
      expect(find.text(candidateName), findsOneWidget);

      final candidateRow = find
          .ancestor(
            of: find.text(candidateName),
            matching: find.byType(Container),
          )
          .first;
      await tester.tap(
        find.descendant(of: candidateRow, matching: find.text('Pair')),
      );

      await tester.runAsync(() async {
        await repo
            .watchPairingSession()
            .firstWhere((s) => s.stage == PairingStage.paired)
            .timeout(const Duration(seconds: 5));
      });

      // Step 3: onboarding auto-advances to Done once paired.
      await settle(tester);
      expect(find.text('Done'), findsOneWidget);
      await tester.tap(find.text('Done'));
      await settle(tester);
      expect(onboardingDone, isTrue);

      late List<Device> devices;
      await tester.runAsync(() async {
        devices = await repo
            .watchDevices()
            .firstWhere((list) => list.any((d) => d.name == candidateName))
            .timeout(const Duration(seconds: 5));
      });
      expect(devices.any((d) => d.name == candidateName), isTrue);

      pairedDeviceName = candidateName;
    },
    timeout: const Timeout(Duration(seconds: 30)),
  );

  testWidgets('state persists across a real daemon restart, visible through a '
      'fresh UI reconnect (SQLite, not process memory)', (tester) async {
    late List<Device> devices;
    late FlowSettings settings;
    await tester.runAsync(() async {
      await restartDaemon();
      devices = await repo.watchDevices().first;
      settings = await repo.watchSettings().first;
    });

    // Desktop (d3) was removed and a new device was paired, both before
    // this restart — identity/pairing persists via SQLite.
    expect(devices.any((d) => d.id == 'd3'), isFalse);
    expect(devices.any((d) => d.name == pairedDeviceName), isTrue);
    // The switch-key setting persists too.
    expect(settings.switchKey.label, 'F13');
    // DeviceState deliberately does *not* persist
    // (`daemon/src/storage/device_repo.rs`): every device reloads as
    // Disconnected until a live connection re-establishes it, so Work
    // Laptop being active before the restart doesn't carry over.
    expect(
      devices.singleWhere((d) => d.id == 'd2').state,
      DeviceState.disconnected,
    );

    await tester.pumpWidget(
      harness(
        const AppWindowShell(
          platform: HostOs.linux,
          initialSection: AppSection.devices,
        ),
      ),
    );
    await settle(tester);

    // The devices themselves — names, not connection state — are what
    // the UI should still show after the restart.
    expect(find.text('Work Laptop'), findsOneWidget);
    expect(find.text('Desktop'), findsNothing);
    expect(find.text(pairedDeviceName!), findsOneWidget);
  });

  testWidgets('the UI survives the daemon process dying mid-session instead of '
      'crashing', (tester) async {
    await tester.pumpWidget(harness(const TrayPopover(platform: HostOs.macos)));
    await settle(tester);
    expect(find.text('Work Laptop'), findsOneWidget);

    await tester.runAsync(() async {
      daemonProcess.kill(ProcessSignal.sigkill);
      await daemonProcess.exitCode.timeout(
        const Duration(seconds: 10),
        onTimeout: () => -1,
      );
    });
    daemonAlreadyStopped = true;

    // Deliberately not asserting *when* (or even whether, within this
    // test's runtime) this command's Future settles: repeated manual
    // probing while writing this suite found that once a process has
    // opened more than one `WebSocketChannel.connect()` (exactly what
    // the restart test above just did), a *later* connection's
    // disconnect detection can take far longer than any UI-relevant
    // bound — well past 40s in one observed run — to notice a killed
    // peer, independent of anything in this app's own code (reproduced
    // with plain `dart:io` sockets, no widgets involved). That's a real
    // gap worth its own investigation, but not one this test can pin
    // down further without single-connection process isolation per
    // test. `unawaited` + `catchError` here still proves the one thing
    // that *is* reliable and is what Tier 0 actually asks for: issuing
    // a command against a dead daemon doesn't throw synchronously and
    // doesn't crash the app, whenever its Future does eventually settle.
    unawaited(repo.switchActiveDevice('d1').catchError((Object _) => null));

    // The app itself must not crash just because the daemon is gone —
    // the UI is allowed to go stale, not throw.
    await settle(tester);
    expect(tester.takeException(), isNull);
    expect(find.text('Work Laptop'), findsOneWidget);
  });
}
