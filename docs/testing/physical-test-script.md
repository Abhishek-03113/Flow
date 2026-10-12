# Flow 2.0 — Physical Test Script (Windows ↔ Mac)

Run by the maintainer on two physical machines on the same LAN. This is the acceptance gate
the automated tests structurally cannot cover: the real OS hooks (Windows low-level hooks,
the macOS event tap, `SendInput`/`CGEventPost`) and watching a keystroke land on the other
screen. Everything above the platform adapters (router, links, pairing, switching, held
keys, edge switching, remaps, every OS pair) is already covered without hardware by
[`harness.md`](harness.md); when something fails here, first check whether the matching
harness scenario passes, which tells you whether the bug is in the engine or in an adapter.

The control model is [ADR 0001](../adr/0001-controller-controlled-input-model.md):

- The **controller** is the machine whose keyboard and mouse you share. It is the only side
  that captures input for forwarding and the only side that switches. Its switch hotkey
  (default **Scroll Lock**) cycles `own screen → controlled machine → own screen`; because the
  controller's capture sees its keyboard even while it hides it from the OS, **the same
  hotkey on the same keyboard always brings control back**.
- A **controlled** machine is driven by a controller. It never captures for forwarding: its
  own keyboard and trackpad keep working locally and are never sent anywhere.
- The role is chosen in onboarding ("Share this computer's keyboard & mouse" / "Control this
  computer from another one") and changed in Settings. Two controllers, or two controlled
  machines, refuse to link (`role_conflict`, shown in the device list).

Round 1 makes **Windows the controller and the Mac controlled** (the everyday setup and the
originally reported bug). Round 2 swaps the roles.

Keep this doc open on a phone or tablet, not on either machine under test.

> **Safety.** Only the controller ever hides its own input, and only while a remote screen is
> active. Scroll Lock on the controller's own keyboard always returns home, and quitting the
> app or daemon releases everything. In Round 2 the Mac is the controller: before starting it,
> open an **SSH session into the Mac** from the Windows PC (`ssh you@mac.local`) and keep
> `killall flow-app flow-daemon` ready; it removes the event tap immediately.

---

## Common setup

### Build and start (each machine)

The app embeds the engine; do not also run `flow-daemon` on the same machine.

```sh
npm --prefix app/ui ci && npm --prefix app/ui run build
cargo run -p flow-app
```

Headless alternative (no UI on that machine; attach a UI later with
`?daemon=ws://127.0.0.1:47823&token=<contents of ~/.flow/ipc.token>`, see `daemon/README.md`):

```sh
# Windows (PowerShell)
$env:FLOW_SECURITY = "insecure"; $env:RUST_LOG = "flow=debug"; cargo run -p flow-daemon
# macOS
FLOW_SECURITY=insecure RUST_LOG=flow=debug cargo run -p flow-daemon
```

`RUST_LOG=flow=debug` gives the product-level trail without dependency noise:
`[PEER] <name> connected|disconnected`, `[SWITCH] <from> -> <to>`,
`[INPUT] <from> -> <to> | KeyDown A`. Use `RUST_LOG=flow=trace` for everything Flow logs.
Capture each machine's output to a file to send back:
`cargo run -p flow-app 2>&1 | tee flow-<machine>-<round>.log`.

### Permissions

- **macOS** asks for **Accessibility** (needed to inject, so needed in both roles) and
  **Input Monitoring** (needed to capture, so only as controller). Grant them in System
  Settings → Privacy & Security (the setup step's **Allow Access** shows both prompts and
  opens the pane). Once both show "Allowed", the screen offers **Restart Flow**: one click
  relaunches it and onboarding resumes at pairing. Pass: no manual quit/reopen needed.
- **Linux** (with `sudo ufw enable` first, to cover the firewall): the setup step lists
  "Read keyboard and mouse", "Create virtual input device" and "Incoming connections".
  **Set Up Automatically** asks for the password once; all three turn "Allowed" within a
  few seconds, with no log out. Pass: `sudo ufw status` lists `47824/udp` and `47825/tcp`,
  and `/etc/udev/rules.d/70-flow.rules` exists. Dismissing the password prompt shows
  "Setup was cancelled"; trying again works.
- **Windows** needs nothing extra, but an elevated (administrator) window does not receive
  injected input from a non-elevated Flow: test with ordinary apps.

---

## Round 0 — Roles, discovery, pairing (nothing is suppressed yet)

**Firewall check (do once):** leave a firewall on for **one** machine only (macOS: System
Settings → Network → Firewall on; Linux: ufw on *without* running the setup). Pair and switch
anyway. Pass: pairing from the other machine reports *why* it failed (names the computer and
`ip:47825`) if it can't connect, and once paired the link still comes up over the one working
direction (`[NET] only one direction of the connection … works; using it` in the log).
Logs: `~/.local/share/flow-daemon/logs/flow.log` (Linux),
`~/Library/Application Support/dev.Flow.flow-daemon/logs/flow.log` (macOS).

1. [ ] Windows: onboarding → **Share this computer's keyboard & mouse** (controller).
2. [ ] Mac: onboarding → **Control this computer from another one** (controlled).
3. [ ] Within ~10 s each side lists the other as a pairing candidate. If not: same subnet?
       Windows Defender Firewall allowing inbound UDP 47824 (`daemon/README.md` "Channels")?
4. [ ] Windows: **Pair** with the Mac. Mac: accept the incoming pairing prompt.
5. [ ] Both device lists show the other as connected; both logs show `[PEER] <name> connected`.
       The Mac's screen appears to the right of the Windows screen in the Arrangement view.
6. [ ] **Role conflict.** Temporarily set the Mac to controller in Settings. → The link drops
       and both device lists show a role-conflict error. Set it back to controlled. → The
       link comes back by itself.
7. [ ] Restart both apps. Pairing, roles and arrangement survive; the link reconnects with no
       re-pair prompt.

**Send back:** both logs and which boxes passed.

---

## Round 1 — Windows controls the Mac

Start state: Round 0 done, Windows controller, Mac controlled, Windows on its own screen.

1. [ ] **Switch.** Press **Scroll Lock** on the Windows keyboard.
       - Windows log: `[SWITCH] <Windows> -> <Mac>`.
       - The Windows UI shows the Mac as the active screen; the Mac UI shows it is being
         controlled by Windows.
       - Scroll Lock itself is typed nowhere and does not toggle the Windows Scroll Lock LED
         into an app.
2. [ ] **Keyboard.** Focus a text field on the Mac. Type `the quick brown fox` on the Windows
       keyboard. → It appears on the **Mac** only; the focused Windows app receives nothing.
3. [ ] **Modifiers.** `Shift`+`a` → `A` on the Mac. `Ctrl`+`c` → note what the Mac does. With a
       remap rule swapping Ctrl and Cmd for Mac targets (Keyboard & Shortcuts → Remap keys),
       `Ctrl`+`c` copies on the Mac.
4. [ ] **Mouse.** Move, left/right/middle click and scroll. → All land on the Mac at its
       cursor; the Windows cursor does not move and Windows apps get nothing.
5. [ ] **Regain control (the original bug).** Press **Scroll Lock** on the Windows keyboard
       again. → `[SWITCH] <Mac> -> <Windows>`; the Windows keyboard and mouse control Windows
       again; nothing more reaches the Mac.
6. [ ] **The Mac stays its own.** While Windows is on its own screen, and again while it
       controls the Mac, type and move on the **Mac's own** keyboard/trackpad. → It always acts
       on the Mac only and never appears on Windows (the "Mac input flows back" bug).
7. [ ] **Edge switching.** Push the Windows pointer through the right edge of its screen. →
       The pointer continues on the Mac. Push back through the Mac's left edge. → It returns to
       Windows at the matching height. Turn edge switching off in Settings → the pointer stops
       at the edge; Scroll Lock still switches.
8. [ ] **Held key across a switch.** On the Mac's screen, hold `j`, press Scroll Lock while
       holding it, then release `j`. → The Mac shows a short `jjj…` that stops at the switch
       (the Mac releases it), Windows types nothing. Then at home hold `k`, switch to the Mac,
       release `k`. → Nothing stuck on either machine.
9. [ ] **Held button.** Start a drag on the Mac, switch home mid-drag. → The drag ends on the
       Mac; no phantom drag or selection on either machine.
10. [ ] **Rapid switching.** Alternate Scroll Lock 20+ times, typing a few characters and
        moving the mouse after each. Every time: input goes only to the screen the last
        `[SWITCH]` named; no stuck keys or buttons; no doubled characters; no crash; both UIs
        always agree on the active screen.
11. [ ] **Release control.** With the Mac active, click **Release control** in the Mac UI. →
        Windows returns to its own screen by itself.
12. [ ] **Disconnect the controlled side while remote.** With the Mac active, quit Flow on
        the Mac (or pull its network). → Windows returns to its own screen **immediately**
        (its keyboard/mouse work locally, nothing stuck), log `[PEER] <Mac> disconnected`,
        the UI shows the Mac reconnecting. Start Flow on the Mac again. → Reconnects within
        ~10 s; Scroll Lock works again.
13. [ ] **Disconnect the controller while remote.** With the Mac active and a key held on
        Windows, quit Flow on Windows. → The Mac releases the key itself (no stuck character)
        and its own keyboard works normally. Restart Windows Flow. → Reconnects; switching works.

**Send back:** both round-1 logs, the checklist results, anything that felt laggy or wrong.

---

## Round 2 — the Mac controls Windows

Change roles in Settings: Mac → controller, Windows → controlled (set the Windows side first;
the link refuses with a role conflict until both are changed, then comes back). Open the SSH
lifeline to the Mac first (see Safety).

This is where macOS *suppression* runs on real hardware: the event tap must drop the Mac's
own events while Windows is active, and the Mac's injected events must not be re-captured.

1. [ ] **One-key pre-flight.** Press Scroll Lock on the Mac keyboard (a Mac keyboard without
       Scroll Lock uses **F13**, set in Keyboard & Shortcuts). Type a single `k`. → It appears
       on Windows and not on the Mac. Switch back with the same key. → The Mac keyboard works
       normally. If the Mac's input feels wedged: `killall flow-app flow-daemon` over SSH.
2. [ ] **Echo check.** Switch to Windows and type `abcdef` on the Mac. → Windows shows exactly
       `abcdef` once.
3. [ ] Repeat Round 1 steps 2–13 with the machines swapped (keyboard, modifiers, mouse,
       regain control, Windows' own keyboard stays local, edge switching, held key and button,
       rapid switching, release control from Windows, both disconnects).
4. [ ] **Tap timeout.** If input ever stalls for ~1 s and resumes, the OS disabled the event
       tap and Flow re-armed it. Note how often; occasional is fine, frequent is a bug.

**Send back:** both round-2 logs, the results, and specifically: did single-key suppression
work, and was there any echo or doubling.

---

## What "pass" means

Every box in Rounds 0–2 checked, with the emphasis from ADR 0001: control can always be
regained from the controller's own keyboard, a controlled machine's input never travels, no
input leaks into the controller while remote, no stuck keys or buttons, no doubled input, no
crash, and both UIs always agree on which screen is active. Partial results are still
valuable: report exactly what passed and what did not, with logs.
