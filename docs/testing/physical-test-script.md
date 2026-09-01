# Flow V1 — Physical Test Script (Windows ↔ Mac)

Run by the maintainer, on two physical machines on the same LAN. This is the acceptance
gate the automated tests structurally cannot cover: watching a keystroke land on the other
screen.

- **Windows PC** — has the physical keyboard and mouse you'll use.
- **Mac** — the second machine.

Both need the repo built: `cargo build -p flow-daemon` (and `flutter build`/`flutter run`
for the UI). Keep this doc open on a phone/tablet, not on either machine under test.

> **Safety — read before Round 2.** Round 2 makes the Mac suppress its own keyboard/mouse.
> If the event tap misbehaves, the Mac's local input can become unresponsive. Before
> starting Round 2: open an **SSH session into the Mac from the Windows PC** (`ssh
> you@mac.local`) and leave it connected. Recovery is `killall flow-daemon` over that SSH
> session — it releases the tap immediately. Windows suppression (Round 1) has the same
> escape: the daemon releases on exit, and `Ctrl-C` in its terminal or Task Manager ends it.

---

## Common setup

### Environment (both daemons)

Development mode, insecure peer security (local dev only), scoped debug logging:

**Windows (PowerShell):**
```powershell
$env:FLOW_ENV       = "development"
$env:FLOW_DEV       = "1"
$env:FLOW_SECURITY  = "insecure"
$env:RUST_LOG       = "flow=debug"
cargo run -p flow-daemon
```

**Mac (bash/zsh):**
```bash
export FLOW_ENV=development FLOW_DEV=1 FLOW_SECURITY=insecure RUST_LOG=flow=debug
cargo run -p flow-daemon
```

`RUST_LOG=flow=debug` (post-logging-fix) gives the product-level trail —
`[PEER] … connected`, `[SWITCH] … -> …`, `[INPUT] … -> … | KeyDown A` — with no
websocket/TCP frame noise. If you need the full firehose for a specific bug, use
`RUST_LOG=flow=trace` instead (still scoped — no dependency noise) or `FLOW_TRACE=1`.

> If `cargo run` rebuilds slowly each start, build once (`cargo build -p flow-daemon`) and
> run `./target/debug/flow-daemon` (Mac) / `.\target\debug\flow-daemon.exe` (Windows)
> directly with the same env vars.

### UI (each machine, optional but recommended for Round-0)

```
# Windows
cd flutter ; flutter run -d windows --dart-define=FLOW_ENV=development --dart-define=FLOW_DAEMON_MODE=ipc
# Mac
cd flutter && flutter run -d macos --dart-define=FLOW_ENV=development --dart-define=FLOW_DAEMON_MODE=ipc
```

macOS will prompt for **Accessibility** and **Input Monitoring** permission for the daemon
(or the terminal running it) — grant both in System Settings → Privacy & Security. Without
them the Mac cannot capture or inject.

### Capturing logs

Redirect each daemon's output to a file so you can send it back:
```
cargo run -p flow-daemon 2>&1 | tee flow-daemon-<machine>-<round>.log
```

---

## Round 0 — Discovery, pairing, connection (both directions safe)

No suppression yet; nothing can lock you out.

1. [ ] Start the Windows daemon. Confirm startup lines: IPC on `127.0.0.1:47823`, peer
       channel on `0.0.0.0:<port>`, discovery on `0.0.0.0:47824`, "announcing to […]".
2. [ ] Start the Mac daemon. Same lines.
3. [ ] Within ~10 s each log shows a discovered peer (`[…] discovery announce parsed` /
       candidate). If not: check both are on the same subnet, and Windows Defender Firewall
       is not blocking inbound UDP 47824 (`daemon/README.md` "Channels" §2).
4. [ ] In the Windows UI: **Pair new device** → the Mac appears → **Pair**.
5. [ ] On the Mac UI: accept the incoming pairing prompt.
6. [ ] Both UIs show the other as a paired device, link state **Connected**. Both logs show
       `[PEER] <name> connected`.
7. [ ] Restart both daemons. Confirm the pairing survived (still listed, reconnects
       automatically) and identity persisted (no re-pair prompt).

**Send back:** both `flow-daemon-*-round0.log` files + which checkboxes passed.

---

## Round 1 — Windows as master (no new code; validates merged Windows suppression)

The Windows PC holds the keyboard/mouse. Target: everything you type/move goes to the Mac
and **not** to Windows; Scroll Lock hands control back.

Start state: paired + connected (Round 0). Make **Windows the active device is wrong** —
you want the **Mac** to be the Active/"Using" device so Windows forwards to it. In the
Windows UI, switch so the Mac shows as active ("Using"), or press Scroll Lock once and
check which way it went.

1. [ ] **Keyboard forward.** Focus a text field on the Mac. Type `the quick brown fox` on
       the Windows keyboard. → The text appears on the **Mac**. → It does **not** appear in
       any Windows field, and the Windows foreground app receives nothing.
       Log (Windows): `[INPUT] <Windows> -> <Mac> | KeyDown t` … per key.
2. [ ] **Modifiers.** On Windows type `Shift`+`a` → `A` on the Mac. `Ctrl`+`c` in a Mac app
       → the Mac sees `Cmd`/`Ctrl`+`c` per its mapping (note what actually happens).
3. [ ] **Mouse move.** Move the Windows mouse. → The **Mac** cursor moves. → The Windows
       cursor does **not** move.
4. [ ] **Clicks.** Left / right / middle click on Mac targets. → All register on the Mac,
       at the cursor position. None leak to Windows.
5. [ ] **Scroll.** Scroll wheel up/down over a Mac scroll view. → Scrolls on the Mac.
6. [ ] **Switch to Windows.** Press **Scroll Lock** once.
       - Log (Windows): `[SWITCH] <Mac> -> <Windows>`, `stage=switch trigger=hotkey`.
       - The Scroll Lock press itself does **not** type anything on the Mac and does
         **not** toggle the Windows Scroll Lock indicator into an app.
       - Now typing/moving on the Windows keyboard/mouse controls **Windows** normally.
         The Mac receives nothing.
7. [ ] **Switch back.** Press **Scroll Lock** again → `[SWITCH] <Windows> -> <Mac>` → back
       to controlling the Mac.
8. [ ] **Repeated switching.** Alternate Scroll Lock 10+ times, typing a few characters and
       moving the mouse after each switch. Confirm every time:
       - no stuck keys (no character repeating forever on either machine),
       - no stuck mouse button (no phantom drag/selection),
       - no duplicate input (a key never lands on both),
       - input always goes to exactly the machine the last `[SWITCH]` named,
       - neither daemon crashes or logs a panic,
       - neither UI's active-device indicator drifts from reality.
9. [ ] **Disconnect.** On the Mac, quit the daemon (`Ctrl-C`). On Windows:
       - `[PEER] <Mac> disconnected` in the log,
       - the Windows keyboard/mouse **immediately** control Windows again (suppression
         released), no stuck keys,
       - UI link state → **Reconnecting**.
10. [ ] **Reconnect.** Restart the Mac daemon → within ~10 s `[PEER] <Mac> connected`,
        link state → Connected, and Scroll Lock switching works again.
11. [ ] **Held key across a switch.** Press and hold `j` on Windows while the Mac is
        active, and while holding it press Scroll Lock. Release `j`. Confirm the Mac does
        **not** get a stuck `j`, and Windows does not either.

**Send back:** `flow-daemon-windows-round1.log`, `flow-daemon-mac-round1.log`, the checklist
results, and a note on anything that felt laggy or wrong. If step 8 or 11 fails, that is
concern #4/#10 territory — the logs are what make it fixable.

---

## Round 2 — Mac as master (after macOS suppression lands — tasks M1–M7)

> **Do the SSH-lifeline step from the top of this doc first.**

_To be finalized once macOS suppression is implemented. It mirrors Round 1 with the roles
reversed: the Mac holds the keyboard/mouse, Windows is the target, and the checklist
verifies the Mac suppresses its own input while forwarding, releases on switch/disconnect,
and never strands a half-pressed key. The first run should start with a single keystroke
and an immediate check that the Mac's own keyboard still works before proceeding._

---

## What "pass" means

All of Round 0, Round 1, and Round 2 checkboxes green, with the specific emphasis from the
brief: no stuck keys, no stuck mouse buttons, no duplicate input, no daemon crash, no
unexplained disconnect, and the UI always reflecting the true master/slave state. Partial
results are still valuable — report exactly what passed and what didn't, with logs.
