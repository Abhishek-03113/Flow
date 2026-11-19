# Flow 3 — onboarding and connection flows, redesigned from expectations

Board task: ACC-26. Status: design, implementation tracked on the Agent Command Center board (ACC-27 onwards).

The owner's report: every suite passes, yet on two real computers Flow does not work. The suites test what
the code does, not what a person expects, and most of them run two engines on one machine over loopback, where
broadcast always arrives, no firewall exists and no OS asks for permission. This document starts again from
the person at two real desks, traces each step to the code that runs today, names the real-machine failure
that the tests cannot see, and specifies the new flow.

## 1. What the person expects

Two computers on the same home or office network, Flow installed on both.

| # | Moment | Expectation |
|---|---|---|
| E1 | First launch | Flow says what it is and asks one question: does this computer share its keyboard and mouse, or is it controlled? |
| E2 | Setup | Everything the OS must allow is listed with **one** action each, and Flow checks for itself whether it worked. On every OS this includes *other computers can reach this one* (firewall, local network privacy). Nothing is reported as ready unless it is. |
| E3 | Finding the other computer | With Flow open on both, each appears on the other within a few seconds, and keeps appearing while the window is open. No "search again" button to mash. |
| E4 | When it does not appear | Flow says what to check (same network, Flow open, setup finished) and offers a way that does not need discovery: type the other computer's address, which Flow shows on screen. |
| E5 | Pairing | Pick the computer, compare a code, accept on the other side. A pairing that cannot work (both set to share, or both controlled) is explained **before** the request, not after. |
| E6 | Connected | Both sides say "Connected". The switch shortcut works straight away. |
| E7 | Daily use | Turn on both computers, they connect by themselves, in any order, whether or not the network carries broadcast. |
| E8 | Interruptions | Wi-Fi drop, sleep and wake, a new DHCP address, a VPN coming up, the other computer rebooting: Flow reconnects within seconds, without the person doing anything. |
| E9 | "Retry" | A retry button retries **now**. |
| E10 | The truth | The status shown is the real state, and every failure names the computer, the step and the fix. |

## 2. What the code does today, and why it fails on real machines

Traced through `daemon/src/engine.rs`, `daemon/src/discovery/tcp.rs`, `daemon/src/service/mod.rs`,
`platform/src/permissions.rs` and `app/ui/src/features/{onboarding,devices}`.

| Expectation | Today | Real-machine failure |
|---|---|---|
| E2 | `platform/src/permissions.rs` Windows `check()` returns an empty list: Windows is "ready" at once. | Windows Defender Firewall blocks inbound UDP 47824 / TCP 47825 by default on Public networks, and dismissing its one-time prompt writes a permanent **block** rule. The Mac never sees the PC, or sees it and cannot connect. Nothing in Flow says so. |
| E2 | macOS: only Accessibility and Input Monitoring are checked. | macOS 15+ *Local Network* privacy: if it is denied (or the prompt was dismissed), every broadcast and LAN connect fails with `EHOSTUNREACH`. Flow only logs it at trace level (`announce_failed`). |
| E3 | Announce destinations are computed **once**, at engine start (`spawn_discovery`). | Flow starts at login, often before Wi-Fi has an address: only `255.255.255.255` is used for the rest of the session, which on a machine with VPN/Hyper-V/Docker adapters leaves on the wrong interface. A new network or VPN later is never picked up. |
| E3 | Pairing "searches" for 1.2 s, then shows whatever announces were cached. | Peers announce every 5 s; the first search usually ends in **"No computers found"**, a dead end with "Search again". |
| E3 | Candidates are keyed by **name** (`live:<name>`), never expire, and known devices are filtered by name. | Two Macs both called "MacBook Pro" collapse into one row; a computer that left stays listed; a new computer with a paired computer's name is never offered. |
| E4 | No way to pair or connect without UDP broadcast. | Networks that drop broadcast — AP client isolation, Wi-Fi to Ethernet bridges on many mesh routers, guest/enterprise networks, a Windows PC on a Public profile — make pairing impossible. |
| E5 | Announces carry no role. | Two "share" computers pair, then the link is refused with `role_conflict`. |
| E7 | A paired peer is redialled **only when its announce arrives** (`handle_discovered_peer`); no address is stored (`devices` table has none). | Any network that drops broadcast, or a firewall that blocks inbound UDP on one side, means two paired computers never reconnect, although TCP between them works. |
| E7 | One-way broadcast is not answered. | If only one side's announces get through (one firewall open, one closed), the side that hears nothing never dials, and the other never hears back. |
| E8 | Recovery relies on the next announce. | After sleep/wake or an IP change, reconnect waits for broadcast; with broadcast blocked it never happens. |
| E8 | `discovery::tcp::spawn_listener` ends on any receive error; the engine loop continues but without a pause. | On Windows a UDP socket reports `WSAECONNRESET` after an ICMP port-unreachable; a persistent error spins a core. |
| E9 | `retry_connection` only changes the displayed state to `connecting`. | The button does nothing until the next announce, if one ever comes. |
| E2/E6 | Onboarding lets the person skip setup, then pairing quietly fails. | The person reaches "Start using Flow" with input or network permissions missing. |

Why the suites did not catch any of it: the harness and the live E2E run every engine on `127.0.0.1` with
`discovery.targets` set to each other's loopback ports, so broadcast, interfaces, firewalls and OS privacy are
never exercised; the mock engine answers pairing on timers.

## 3. The new design

### 3.1 Principles

1. **Discovery is a hint, not the connection.** Broadcast helps two computers *find* each other once. After
   pairing, each side remembers where the other was and dials it directly; broadcast only updates that address.
2. **Answer what you hear.** An announce from another Flow is answered with a unicast announce, so one working
   direction is enough for both sides to learn each other.
3. **Re-read the network all the time.** Interfaces and their broadcast addresses are enumerated on every
   announce tick, so a late Wi-Fi, a new network or a VPN is picked up within one interval.
4. **Identity, not names.** A candidate is the computer's identity key (already in the announce as
   `instance_id`). Names are labels.
5. **Nothing is "ready" unless Flow checked it.** Each OS lists *Incoming connections* as a setup item that is
   probed for real and fixed with one action.
6. **Every button does what it says, now.**

### 3.2 Discovery (engine)

- Announce packet v2 adds `role` and `proto` (protocol version). Old peers without them still parse.
- Every tick: re-enumerate interfaces, send to every subnet broadcast plus `255.255.255.255`, **plus unicast to
  the last known address of every paired computer that is not linked**.
- On receiving an announce from another instance: answer with one unicast announce to the sender, at most once
  per sender per interval.
- Receive errors: Windows `SIO_UDP_CONNRESET` off; any error pauses 250 ms before the next receive.
- Send errors: when every destination fails with "no route"/"permission denied" for several ticks, report a
  `Local network` problem as a permission item (macOS Local Network privacy, sandbox, no network).

### 3.3 Pairing candidates

- Keyed by identity key: `live:<instance_id>`. Expire 15 s after the last announce.
- Filtered out when the identity belongs to a paired device (`device_id_from_public_key`), never by name.
- Carry `role`; the UI marks a same-role candidate and explains the fix before any request.
- The search does not end: `found` is entered at once and the list updates live while pairing is open.

### 3.4 Connect by address (no discovery needed)

- New command `pair_with_address { address }` (`host` or `host:port`, default port 47825): same Noise pairing
  as a discovered candidate.
- The UI shows this computer's own LAN addresses (new `local_addresses` in the status snapshot) so the person
  can read one off one screen and type it on the other.

### 3.5 Reconnect supervisor (engine)

- Each paired device stores `last_address` (schema migration) whenever a link comes up, and from every announce
  of that identity.
- A supervisor task per engine: every second, for each paired device that is not linked and not refused, dial
  its last address when its backoff allows (1 s, 2 s, 4 s … capped at 15 s; reset on success or on network
  change). Inbound links still win the existing precedence rule.
- `retry_connection` resets every backoff and dials immediately.

### 3.6 Setup items per OS

| OS | Item | Probe | One action |
|---|---|---|---|
| Windows | Incoming connections | `netsh advfirewall firewall show rule name="Flow"` lists the allow rules for UDP 47824 and TCP 47825, or the firewall is off for the active profile | One UAC prompt adding both rules (all profiles), then re-probe |
| macOS | Accessibility, Input Monitoring | as today | as today |
| macOS | Local network | discovery send results (3.2) | open Privacy & Security › Local Network |
| Linux | as today (udev, uinput, ufw/firewalld) | | |

### 3.7 Onboarding

1. **Role** — unchanged.
2. **Setup** — every item above; "Skip for now" stays but the pairing step then shows a banner naming what is
   missing and that pairing may not work until it is fixed.
3. **Pair** — live list of nearby computers (role shown); after 10 s with nothing found, a checklist (same
   network, Flow open and set up on the other computer) and **Connect by address** with this computer's own
   address shown. Done when a link is actually up, on either side.

### 3.8 States, timeouts and what the person sees

| State | Enters when | Leaves when | Person sees |
|---|---|---|---|
| Searching | pairing opened | pairing closed | live list, or after 10 s the checklist and address entry |
| Requesting | Connect pressed | accept (paired), refuse, 30 s decision timeout, connect 6 s / handshake 10 s timeout | code, "waiting for X to accept"; each failure names step and fix (already implemented) |
| Paired, not linked | pairing done or startup | link up | "Connecting to X…" with the address being tried |
| Linked | Hello exchanged | 7 s silence, close, role change | "Connected" |
| Reconnecting | link lost | link up | "Reconnecting to X…", Retry dials now |

## 4. Implementation tasks (board)

| Task | Scope | Priority |
|---|---|---|
| ACC-27 | Discovery hardening: per-tick interfaces, unicast answers, error handling, announce v2 (role), candidates by identity with expiry | urgent |
| ACC-28 | Reconnect supervisor: stored last address, direct redial with backoff, real `retry_connection` | urgent |
| ACC-29 | Windows firewall setup item and macOS local-network detection | urgent |
| ACC-30 | Connect by address + local addresses in the snapshot | high |
| ACC-31 | Pairing and onboarding UI on the new flows | high |
| ACC-32 | Tests from expectations: harness scenarios with broadcast blocked, real-machine script update | high |

## 5. Verification limits

This container has one machine, no Windows or macOS, no real LAN and no firewall. Everything here is verified
by unit tests, two engines over real sockets with discovery deliberately cut, and cross-target compilation.
The physical test script (`docs/testing/physical-test-script.md`) is updated so the first run on real machines
checks each expectation E1–E10 explicitly.
