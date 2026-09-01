//! Structured logging setup (`daemon/todos.json` I2): wraps
//! `tracing-subscriber`'s reload mechanism — confined to this module per
//! the wrap-third-party-deps rule — so `settings.debug_logging` can
//! change the daemon's log verbosity at runtime, without a restart,
//! instead of the filter level being fixed for the whole process
//! lifetime the way `tracing_subscriber::fmt::init()` alone leaves it.

use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use tracing_subscriber::prelude::*;
use tracing_subscriber::reload;

use crate::service::DaemonService;

/// The three level names the scoped filter directives are ever built
/// from. `EnvFilter` directives are textual, so `set_debug`/`init` render
/// the chosen [`LevelFilter`] back to one of these.
fn level_str(level: LevelFilter) -> &'static str {
    match level {
        LevelFilter::TRACE => "trace",
        LevelFilter::DEBUG => "debug",
        _ => "info",
    }
}

/// The scoped directive set the daemon runs with whenever `RUST_LOG` is
/// *not* set: product logs (`target: "flow"`) and the daemon's own
/// crate-path records (`flow_daemon…`) at `level`, and nothing else — so
/// a `tokio_tungstenite` / `tungstenite` TCP-frame `TRACE` firehose
/// reaching `tracing` through the `tracing-log` bridge stays filtered
/// out even under `FLOW_TRACE=1`.
fn scoped_directives(level: LevelFilter) -> String {
    let l = level_str(level);
    format!("flow={l},flow_daemon={l}")
}

/// The tracing target every end-to-end lifecycle record is emitted on.
/// Grep a log for this string to isolate the trail: discovery → dial /
/// accept → Noise handshake → pairing → pipeline up → per-event send /
/// gate / recv / inject → switch → pipeline down. Documented as a const
/// for reference; the macros below hard-code the same literal because
/// `tracing`'s `target:` only accepts one.
pub const HOP_TARGET: &str = "flow::hop";

/// One per-event lifecycle record — the firehose (capture, send-gate,
/// frame out, frame in, replay drop, inject). `TRACE` level, so it is
/// silent unless `FLOW_TRACE=1`. Always pass a `stage` and, where they
/// apply, `role` (`owner` / `receiver` / `initiator` / `responder` /
/// `local`), `peer`, `seq`, `kind`.
#[macro_export]
macro_rules! hop {
    ($($arg:tt)*) => {
        tracing::trace!(target: "flow::hop", $($arg)*)
    };
}

/// A milestone lifecycle record — connection came up / went down,
/// pairing decided, active device switched, suppression toggled. `DEBUG`
/// level so `settings.debug_logging` (or `FLOW_TRACE`) surfaces it
/// without the full per-event firehose.
#[macro_export]
macro_rules! hop_note {
    ($($arg:tt)*) => {
        tracing::debug!(target: "flow::hop", $($arg)*)
    };
}

/// A live handle to the daemon's log filter level. Cheap to clone —
/// every clone controls the same underlying filter, the same sharing
/// model `Storage`'s own handle already uses.
#[derive(Clone)]
pub struct LoggingHandle {
    reload_handle: reload::Handle<EnvFilter, tracing_subscriber::Registry>,
    /// Whether `FLOW_TRACE` pinned the verbose `TRACE` floor on for the
    /// whole run, so a later `settings.debug_logging = false` coming
    /// through the toggle can't quietly turn the trace firehose back off
    /// mid-session — [`Self::set_debug`] stays at `TRACE` regardless of
    /// what it's asked for.
    trace_floor: bool,
    /// `true` when `RUST_LOG` was set in the environment at [`init`] time.
    /// The operator has then spelled the filter out themselves, so
    /// [`Self::set_debug`] must not fight them — the `debug_logging`
    /// toggle becomes an inert no-op for the rest of the run.
    env_controlled: bool,
}

impl LoggingHandle {
    /// Sets the effective log level on the scoped `flow` / `flow_daemon`
    /// filter: `true` shows `debug`-and-above — matching
    /// `docs/product/vision.md` §15's Advanced settings "Debug logging"
    /// toggle actually doing something once wired to a real daemon;
    /// `false` shows `info`-and-above only. A [`trace_floor`](Self) run
    /// (`FLOW_TRACE=1`) stays pinned at `TRACE` regardless of the
    /// persisted `debug_logging` setting. When `RUST_LOG` was set at
    /// [`init`] time the operator owns the filter, so this is an inert
    /// no-op (logged once at `debug`).
    /// A `SubscriberGone` failure (the global subscriber was somehow
    /// torn down) is logged, not panicked on — logging misconfiguration
    /// shouldn't take the daemon down.
    pub fn set_debug(&self, enabled: bool) {
        if self.env_controlled {
            tracing::debug!("log level is controlled by RUST_LOG; ignoring debug_logging toggle");
            return;
        }
        let level = if self.trace_floor {
            LevelFilter::TRACE
        } else if enabled {
            LevelFilter::DEBUG
        } else {
            LevelFilter::INFO
        };
        if self
            .reload_handle
            .reload(EnvFilter::new(scoped_directives(level)))
            .is_err()
        {
            tracing::warn!("could not reload the log filter: the subscriber is gone");
        }
    }

    /// The highest verbosity the live filter will currently enable.
    /// Mainly for this module's own tests; also a reasonable diagnostic.
    /// Falls back to `INFO` if the subscriber is gone or the filter
    /// offers no hint.
    pub fn max_level(&self) -> LevelFilter {
        self.reload_handle
            .with_current(|filter| filter.max_level_hint())
            .ok()
            .flatten()
            .unwrap_or(LevelFilter::INFO)
    }
}

/// Installs the global tracing subscriber and returns a handle that can
/// change its filter level later. Must be called exactly once, as early
/// as possible in `main` — before `settings.debug_logging`'s persisted
/// value is even known, so callers sync the real value onto the
/// returned handle once it's loaded (see `daemon/README.md`'s "Auto-reconnect"-
/// adjacent structured-logging section for the full startup sequence).
pub fn init(debug_logging: bool, trace: bool) -> LoggingHandle {
    let base = if trace {
        LevelFilter::TRACE
    } else if debug_logging {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };
    // `RUST_LOG`, when set, wins verbatim — the operator has spelled the
    // filter out and we defer to it entirely, only supplying `base` as
    // the fallback directive for targets `RUST_LOG` doesn't mention.
    // Otherwise the daemon runs the tight `flow` / `flow_daemon` scoped
    // set, so dependency `TRACE` noise never reaches the console even
    // under `FLOW_TRACE=1`.
    let env_controlled = std::env::var_os("RUST_LOG").is_some();
    let initial = if env_controlled {
        EnvFilter::builder()
            .with_default_directive(base.into())
            .from_env_lossy()
    } else {
        EnvFilter::new(scoped_directives(base))
    };
    let (filter, reload_handle) = reload::Layer::new(initial);
    tracing_subscriber::registry()
        .with(filter)
        .with(
            // Under `FLOW_TRACE` every hop record wants to be
            // attributable: which thread, which module target, which
            // line. Off by default so a normal run's logs stay terse.
            tracing_subscriber::fmt::layer()
                .with_thread_ids(trace)
                .with_target(trace)
                .with_line_number(trace),
        )
        .init();
    LoggingHandle {
        reload_handle,
        trace_floor: trace,
        env_controlled,
    }
}

/// Syncs `logging`'s filter level to `service`'s persisted
/// `settings.debug_logging` immediately (`init`'s own caller only ever
/// had a hardcoded starting guess before settings actually loaded),
/// then keeps it in sync as `DaemonService::update_settings` changes it
/// — `daemon/todos.json` I2's "toggling debug_logging via
/// update_settings visibly changes log verbosity in a manual run"
/// acceptance criterion, made real rather than aspirational.
pub fn spawn_debug_logging_toggle(
    service: &DaemonService,
    logging: LoggingHandle,
) -> tokio::task::JoinHandle<()> {
    let mut settings_rx = service.watch_settings();
    logging.set_debug(settings_rx.borrow_and_update().debug_logging);
    tokio::spawn(async move {
        while settings_rx.changed().await.is_ok() {
            logging.set_debug(settings_rx.borrow_and_update().debug_logging);
        }
    })
}

/// Product-level log lines — the short, plain-language trail of what the
/// daemon is actually doing (input flowing, the active device switching,
/// peers coming and going, errors) as opposed to the `flow::hop`
/// lifecycle firehose. Every line is emitted on the bare `"flow"` target,
/// which the scoped `flow=…` directive covers, so `RUST_LOG=flow=debug`
/// (or the default scoped filter) surfaces them with no dependency noise
/// and without needing `RUST_LOG=trace`.
///
/// The ASCII arrow `->` is deliberate: a Unicode arrow renders as mojibake
/// on a default Windows console code page.
pub mod product {
    /// One forwarded input event, at `DEBUG` — this fires per event, so
    /// it would flood a normal `INFO` run. `from`/`to` are display names.
    pub fn input(from: &str, to: &str, detail: &str) {
        tracing::debug!(target: "flow", "[INPUT] {from} -> {to} | {detail}");
    }

    /// The active device changed: input now flows toward `to`.
    pub fn switch(from: &str, to: &str) {
        tracing::info!(target: "flow", "[SWITCH] {from} -> {to}");
    }

    /// A paired peer's streaming pipeline came up.
    pub fn peer_connected(name: &str) {
        tracing::info!(target: "flow", "[PEER] {name} connected");
    }

    /// A paired peer's streaming pipeline went away.
    pub fn peer_disconnected(name: &str) {
        tracing::info!(target: "flow", "[PEER] {name} disconnected");
    }

    /// A product-visible failure: `context` says what was being attempted,
    /// `err` is the underlying error rendered with `{:?}`.
    pub fn error(context: &str, err: &dyn std::fmt::Debug) {
        tracing::error!(target: "flow", "[ERROR] {context}: {err:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a `LoggingHandle` over a real `reload::Layer` kept alive
    /// locally (as part of `_subscriber`) without installing it as the
    /// process's global default — `tracing_subscriber::registry().init()`
    /// can only ever run once per process, which would make every test
    /// after the first panic if this test tried to actually install it.
    /// `Handle::reload`/`::with_current` only need the layer's
    /// underlying `Arc` to still be alive somewhere, which keeping
    /// `_subscriber` in scope guarantees.
    fn a_handle_with_a_kept_alive_layer(initial: LevelFilter) -> (LoggingHandle, impl Sized) {
        a_handle_with_floor(initial, false)
    }

    fn a_handle_with_floor(initial: LevelFilter, trace_floor: bool) -> (LoggingHandle, impl Sized) {
        let (filter, reload_handle) =
            reload::Layer::new(EnvFilter::new(scoped_directives(initial)));
        let subscriber = tracing_subscriber::registry().with(filter);
        (
            LoggingHandle {
                reload_handle,
                trace_floor,
                env_controlled: false,
            },
            subscriber,
        )
    }

    #[test]
    fn a_trace_floor_keeps_set_debug_false_from_dropping_below_trace() {
        let (handle, _subscriber) = a_handle_with_floor(LevelFilter::TRACE, true);
        handle.set_debug(false);
        assert_eq!(handle.max_level(), LevelFilter::TRACE);
        handle.set_debug(true);
        assert_eq!(handle.max_level(), LevelFilter::TRACE);
    }

    #[test]
    fn without_a_trace_floor_set_debug_still_toggles_info_and_debug() {
        let (handle, _subscriber) = a_handle_with_floor(LevelFilter::INFO, false);
        handle.set_debug(true);
        assert_eq!(handle.max_level(), LevelFilter::DEBUG);
        handle.set_debug(false);
        assert_eq!(handle.max_level(), LevelFilter::INFO);
    }

    #[test]
    fn set_debug_true_reloads_to_the_debug_level() {
        let (handle, _subscriber) = a_handle_with_a_kept_alive_layer(LevelFilter::INFO);
        handle.set_debug(true);
        assert_eq!(handle.max_level(), LevelFilter::DEBUG);
    }

    #[test]
    fn set_debug_false_reloads_to_the_info_level() {
        let (handle, _subscriber) = a_handle_with_a_kept_alive_layer(LevelFilter::DEBUG);
        handle.set_debug(false);
        assert_eq!(handle.max_level(), LevelFilter::INFO);
    }

    #[test]
    fn toggling_back_and_forth_leaves_the_level_matching_the_last_call() {
        let (handle, _subscriber) = a_handle_with_a_kept_alive_layer(LevelFilter::INFO);
        handle.set_debug(true);
        handle.set_debug(true);
        handle.set_debug(false);
        assert_eq!(handle.max_level(), LevelFilter::INFO);
        handle.set_debug(true);
        assert_eq!(handle.max_level(), LevelFilter::DEBUG);
    }

    /// With `RUST_LOG` in force at `init` time, the persisted
    /// `debug_logging` toggle must not fight the operator's explicit
    /// filter: `set_debug` becomes an inert no-op.
    #[test]
    fn rust_log_set_makes_set_debug_a_noop() {
        let (filter, reload_handle) =
            reload::Layer::new(EnvFilter::new(scoped_directives(LevelFilter::INFO)));
        let _subscriber = tracing_subscriber::registry().with(filter);
        let handle = LoggingHandle {
            reload_handle,
            trace_floor: false,
            env_controlled: true,
        };
        let before = handle.max_level();
        handle.set_debug(true);
        assert_eq!(handle.max_level(), before);
        assert_eq!(handle.max_level(), LevelFilter::INFO);
    }

    /// This task's own acceptance criterion, exercised for real: a
    /// `DaemonService::update_settings` call actually reaches the log
    /// filter, through the exact wiring `main.rs` installs — no
    /// simulated settings change, no direct call to `set_debug`.
    #[tokio::test]
    async fn update_settings_toggles_the_log_filter_through_a_real_daemon_service() {
        use crate::storage::Storage;
        use flow_core::settings::SettingsPatch;

        let (handle, _subscriber) = a_handle_with_a_kept_alive_layer(LevelFilter::INFO);
        let storage = Storage::open_in_memory().await.expect("open in-memory db");
        let service = DaemonService::new_seeded_for_test(storage).await;

        // `debug_logging` defaults to `false` (`FlowSettings::defaults`),
        // so spawning the toggle should leave the filter at `INFO`.
        let _toggle = spawn_debug_logging_toggle(&service, handle.clone());
        assert_eq!(handle.max_level(), LevelFilter::INFO);

        service
            .update_settings(SettingsPatch {
                debug_logging: Some(true),
                ..Default::default()
            })
            .await
            .expect("update settings");
        // `update_settings` already awaited its own `send_replace`, but
        // the toggle task still needs a scheduling turn to observe it.
        tokio::task::yield_now().await;
        assert_eq!(handle.max_level(), LevelFilter::DEBUG);

        service
            .update_settings(SettingsPatch {
                debug_logging: Some(false),
                ..Default::default()
            })
            .await
            .expect("update settings");
        tokio::task::yield_now().await;
        assert_eq!(handle.max_level(), LevelFilter::INFO);
    }
}
