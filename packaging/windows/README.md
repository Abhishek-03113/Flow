# Windows service (not yet implemented)

This is a documentation-only stub (`daemon/todos.json` J4): there is no Windows service wrapper, installer, or unit file in this repository yet. It records the intended approach and its open design question so a future task has a concrete starting point, matching `daemon/README.md`'s "Process supervision" section.

## Two viable approaches

1. **A wrapper service manager** — [NSSM](https://nssm.cc/) (the Non-Sucking Service Manager) or [WinSW](https://github.com/winsw/winsw) wrap an arbitrary executable (`flow-daemon.exe`) as a real Windows Service without writing any Win32 service code: `nssm install FlowDaemon C:\path\to\flow-daemon.exe`, with a failure-actions policy (`nssm set FlowDaemon AppExit Default Restart`, or equivalently via `sc.exe failure`) giving the same "restart on crash, not on a clean exit" behavior `daemon/README.md`'s launchd/systemd notes describe. Lowest implementation effort; adds a third-party runtime dependency for end users (bundling the wrapper binary with an installer).
2. **A real Win32 service**, implemented directly with the [`windows`](https://docs.rs/windows) crate `flow-platform`'s own Windows input adapters (E6/E7) already depend on — registering `flow-daemon.exe` itself as a `SERVICE_WIN32_OWN_PROCESS` service via the Service Control Manager APIs (`StartServiceCtrlDispatcherW`, a `LPSERVICE_MAIN_FUNCTION` entry point, `SetServiceStatus` for start/stop/crash reporting). No extra runtime dependency, but meaningfully more code: a Windows service needs its own event loop shape (a `ServiceMain` callback, not a plain `fn main`), distinct from how `flow-daemon` runs today.

Neither is implemented in this pass — both are recorded here as the real options a future task should choose between, not a recommendation being acted on now.

## The open design question: Session 0 isolation

A genuine Windows Service runs in **Session 0**, a non-interactive session with no desktop, no logged-in user, and critically, no access to the input APIs `flow-platform`'s Windows adapters use (`SetWindowsHookEx` for capture, `SendInput` for injection — both are scoped to the interactive desktop of a real user session). A service running in Session 0 **cannot capture or inject input at all** — this isn't a permissions problem to work around, it's how Windows session isolation is designed to work since Windows Vista.

This means the macOS (`launchd` `LaunchAgent`, per-user) and Linux (`systemd` user unit) approaches this project already documents don't have a literal Windows equivalent: those both run *in* the user's own session by design, where a Windows Service structurally cannot. The practical options, to be resolved by whichever task actually implements this:

- **Don't use a Windows Service at all** — instead, an entry in the user's `Run` registry key (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) or a Startup-folder shortcut, giving genuine "start at login, in the user's session" behavior at the cost of losing the Service Control Manager's own crash-restart/monitoring machinery (which would need to be reimplemented, e.g. a small always-running watchdog, or accepted as a gap).
- **A hybrid**: a Session-0 Windows Service whose only job is to detect the active console session (`WTSGetActiveConsoleSessionId`) and launch/supervise the *real* `flow-daemon` process inside that user's session via `CreateProcessAsUser` — meaningfully more complex, but keeps genuine OS-level supervision.

This is flagged explicitly as unresolved rather than picking one silently — the tradeoffs affect both the implementation effort and what "flow-daemon crashed, did it restart" actually means on Windows, and deserve a real decision when J4 is picked back up for a full implementation, not an assumption baked in during scaffolding.
