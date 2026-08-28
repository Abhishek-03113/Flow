// Dev-only headless IPC client for driving a running flow-daemon during
// manual verification. NOT part of the app or the test suite.
//
// Usage:
//   dart run tool/ipc_poke.dart <tokenPath> <port> [--seconds N] \
//       [--send <command> [jsonPayload]] [--send ...] [--at <ms> --send ...]
//
// Streams every event frame for the whole run. Each --send is dispatched
// after the preceding --at delay (default 300ms after connect / after the
// previous send). Example — drive a full pair from the initiator side:
//
//   dart run tool/ipc_poke.dart TOKEN 47823 --seconds 12 \
//     --send start_pairing --at 2000 --send pair_with_candidate "{\"candidate_id\":\"live:NAME\"}"

import 'dart:async';
import 'dart:convert';
import 'dart:io';

Future<void> main(List<String> args) async {
  if (args.length < 2) {
    stderr.writeln(
      'usage: ipc_poke.dart <tokenPath> <port> [--seconds N] [--at ms] [--send cmd [payload]]...',
    );
    exit(2);
  }
  final tokenPath = args[0];
  final port = int.parse(args[1]);

  var holdSeconds = 8;
  final sends = <({int delayMs, String command, dynamic payload})>[];
  var pendingDelay = 300;
  for (var i = 2; i < args.length; i++) {
    switch (args[i]) {
      case '--seconds':
        holdSeconds = int.parse(args[++i]);
      case '--at':
        pendingDelay = int.parse(args[++i]);
      case '--send':
        final command = args[++i];
        dynamic payload;
        if (i + 1 < args.length && !args[i + 1].startsWith('--')) {
          payload = jsonDecode(args[++i]);
        }
        sends.add((delayMs: pendingDelay, command: command, payload: payload));
        pendingDelay = 300;
      default:
        stderr.writeln('unknown arg: ${args[i]}');
        exit(2);
    }
  }

  final token = File(tokenPath).readAsStringSync().trim();
  final ws = await WebSocket.connect(
    'ws://127.0.0.1:$port',
    protocols: [token],
  );
  stdout.writeln('connected to 127.0.0.1:$port');

  ws.listen(
    (dynamic data) {
      final json = jsonDecode(data as String) as Map<String, dynamic>;
      final ts = DateTime.now().toIso8601String().substring(11, 23);
      if (json['event'] != null) {
        stdout.writeln(
          '$ts EVENT ${json['event']}: ${jsonEncode(json['payload'])}',
        );
      } else {
        stdout.writeln('$ts REPLY: ${jsonEncode(json)}');
      }
    },
    onDone: () => stdout.writeln('socket closed'),
    onError: (Object e) => stderr.writeln('socket error: $e'),
  );

  var id = 0;
  unawaited(() async {
    for (final s in sends) {
      await Future<void>.delayed(Duration(milliseconds: s.delayMs));
      final frame = jsonEncode({
        'id': 'poke-${id++}',
        'command': s.command,
        'payload': s.payload,
      });
      stdout.writeln(
        '${DateTime.now().toIso8601String().substring(11, 23)} SEND: $frame',
      );
      ws.add(frame);
    }
  }());

  await Future<void>.delayed(Duration(seconds: holdSeconds));
  await ws.close();
}
