# Keymaps

Flow translates keys between operating systems by **name**. Each OS has a
table (`windows.toml`, `macos.toml`, `linux.toml`) mapping its native key
codes to the shared names in `flow_core::protocol::key_names` (`RETURN`,
`COMMAND`, `LEFT_ARROW`, letters `A`..`Z`, digits `0`..`9`, `F1`..`F20`, ...).

- **Capture** (this machine's keyboard -> wire): native code -> name.
- **Injection** (wire -> this machine's keyboard): name -> native code.

The files here are the defaults; they are embedded in the binaries at compile
time. You can change any mapping, and add remap rules, **without rebuilding**
by dropping files in your override directory.

| What | Native code is |
|---|---|
| `windows.toml` | Win32 virtual-key code (`VK_*`) |
| `macos.toml` | `CGKeyCode` (Apple `kVK_*`) |
| `linux.toml` | evdev key code (`KEY_*` from `input-event-codes.h`) |

Capture and injection share one code space per OS, so there is one `code`
column.

## File format

```toml
os = "linux"     # windows | macos | linux
version = 1      # optional in override files

keys = [
  { name = "RETURN", code = 28 },
  { name = "CAPS_LOCK", code = 58 },
  { name = "APPS", code = 0x5D, local = true },
  { name = "ESCAPE", code = 1, aliases = ["ESC"] },
]
```

| Field | Meaning |
|---|---|
| `name` | Wire name. Must be in the shared vocabulary unless `local = true`. |
| `code` | Native code; decimal or `0x` hex. |
| `aliases` | Extra names that *inject* this key. Never produced on capture. |
| `local` | A key with no cross-platform name (Windows' menu key `APPS`, `F21`..). Skips the vocabulary check; other OSes can't produce or understand it. |

Validation (a failing file is ignored with a warning, never a crash): every
name must be in the vocabulary (or `local`), and a name/alias or a code may
appear only once.

Codes missing from the table still work: Windows and macOS send/accept a hex
literal (`"0x5D"`), Linux falls back to evdev's own `KEY_<NAME>` spelling.

## Overriding

Override files live in the `keymaps` folder of the daemon's config
directory (`FLOW_CONFIG_DIR` if set; otherwise `~/.config/flow-daemon` on
Linux, `~/Library/Application Support/dev.Flow.flow-daemon` on macOS,
`%APPDATA%\Flow\flow-daemon\config` on Windows). `flow_platform::keymap::overrides_dir()`
returns the exact path; the app's "reveal config dir" opens it.

Create `<os>.toml` there with only the entries you want to change. Each
override entry **replaces** any default entry with the same `code` or the same
`name`/alias; everything else keeps its default. Restart the daemon to apply.

Example, `linux.toml`: make Caps Lock send Escape and add a second name for
Enter:

```toml
os = "linux"
keys = [
  { name = "ESCAPE", code = 58 },                     # Caps Lock position now types ESCAPE
  { name = "RETURN", code = 28, aliases = ["ENTER"] },
]
```

(The default `ESCAPE` at code 1 and `CAPS_LOCK` at code 58 are displaced.)

If the file is invalid, the daemon logs `ignoring <path>: <reason>; using
defaults` and carries on with the embedded keymap.

## Adding a key

1. If it has a cross-platform meaning, add its name to
   `core/src/protocol/key_names.rs` (constant and `all()`), then add an entry
   to each of the three default files. The vocabulary is code because it is
   the wire protocol: a name that isn't in it is rejected.
2. If it only exists on one OS, add `{ name = "...", code = ..., local = true }`
   to that OS's file (or your override file).

Tests (`cargo test -p flow-core keymap`) check that every embedded file parses,
covers the vocabulary, and round-trips in both directions.

## Remap rules

`remap.toml` (same directory) rewrites keys on the **controller** before
they are forwarded, per target OS:

```toml
[[rule]]
from = "Ctrl"
to = "Meta"
when_target_os = "macos"     # optional: macos | windows | linux

[[rule]]
from = "Meta"
to = "Ctrl"
when_target_os = "macos"
```

- `from`/`to` are both **modifier tokens** (`Shift`, `Ctrl`, `Alt`, `Meta`) or
  both **key names** (`CAPS_LOCK`, `ESCAPE`, ...). A modifier rule remaps the
  left and right keys *and* the event's modifier list; a key rule remaps only
  that key.
- Rules never chain: each key is looked up once against the original event, so
  the two rules above swap Ctrl and Cmd instead of cancelling out. The first
  matching rule wins.
- Omit `when_target_os` to apply to every target.

An invalid `remap.toml` is ignored with a warning. The default
(`config/keymaps/remap.toml`) contains only commented examples.
