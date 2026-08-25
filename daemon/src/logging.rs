//! Structured logging setup (`daemon/todos.json` I2): wraps
//! `tracing-subscriber`'s reload mechanism — confined to this module per
//! the wrap-third-party-deps rule — so `settings.debug_logging` can
//! change the daemon's log verbosity at runtime, without a restart,
//! instead of the filter level being fixed for the whole process
//! lifetime the way `tracing_subscriber::fmt::init()` alone leaves it.

use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::reload;

use crate::service::DaemonService;

/// A live handle to the daemon's log filter level. Cheap to clone —
/// every clone controls the same underlying filter, the same sharing
/// model `Storage`'s own handle already uses.
#[derive(Clone)]
pub struct LoggingHandle {
    reload_handle: reload::Handle<LevelFilter, tracing_subscriber::Registry>,
}

impl LoggingHandle {
    /// Sets the effective log level: `true` shows `debug`-and-above
    /// spans/events — matching `docs/product/vision.md` §15's Advanced
    /// settings "Debug logging" toggle actually doing something once
    /// wired to a real daemon; `false` shows `info`-and-above only.
    /// A `SubscriberGone` failure (the global subscriber was somehow
    /// torn down) is logged, not panicked on — logging misconfiguration
    /// shouldn't take the daemon down.
    pub fn set_debug(&self, enabled: bool) {
        let level = if enabled {
            LevelFilter::DEBUG
        } else {
            LevelFilter::INFO
        };
        if self.reload_handle.reload(level).is_err() {
            tracing::warn!("could not reload the log filter: the subscriber is gone");
        }
    }

    /// The currently active level. Mainly for this module's own tests;
    /// also a reasonable diagnostic if ever needed.
    pub fn current_level(&self) -> LevelFilter {
        self.reload_handle
            .with_current(|level| *level)
            .unwrap_or(LevelFilter::INFO)
    }
}

/// Installs the global tracing subscriber and returns a handle that can
/// change its filter level later. Must be called exactly once, as early
/// as possible in `main` — before `settings.debug_logging`'s persisted
/// value is even known, so callers sync the real value onto the
/// returned handle once it's loaded (see `daemon/README.md`'s "Auto-reconnect"-
/// adjacent structured-logging section for the full startup sequence).
pub fn init(debug_logging: bool) -> LoggingHandle {
    let initial = if debug_logging {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };
    let (filter, reload_handle) = reload::Layer::new(initial);
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
    LoggingHandle { reload_handle }
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
        let (filter, reload_handle) = reload::Layer::new(initial);
        let subscriber = tracing_subscriber::registry().with(filter);
        (LoggingHandle { reload_handle }, subscriber)
    }

    #[test]
    fn set_debug_true_reloads_to_the_debug_level() {
        let (handle, _subscriber) = a_handle_with_a_kept_alive_layer(LevelFilter::INFO);
        handle.set_debug(true);
        assert_eq!(handle.current_level(), LevelFilter::DEBUG);
    }

    #[test]
    fn set_debug_false_reloads_to_the_info_level() {
        let (handle, _subscriber) = a_handle_with_a_kept_alive_layer(LevelFilter::DEBUG);
        handle.set_debug(false);
        assert_eq!(handle.current_level(), LevelFilter::INFO);
    }

    #[test]
    fn toggling_back_and_forth_leaves_the_level_matching_the_last_call() {
        let (handle, _subscriber) = a_handle_with_a_kept_alive_layer(LevelFilter::INFO);
        handle.set_debug(true);
        handle.set_debug(true);
        handle.set_debug(false);
        assert_eq!(handle.current_level(), LevelFilter::INFO);
        handle.set_debug(true);
        assert_eq!(handle.current_level(), LevelFilter::DEBUG);
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
        let service = DaemonService::new(storage).await;

        // `debug_logging` defaults to `false` (`FlowSettings::defaults`),
        // so spawning the toggle should leave the filter at `INFO`.
        let _toggle = spawn_debug_logging_toggle(&service, handle.clone());
        assert_eq!(handle.current_level(), LevelFilter::INFO);

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
        assert_eq!(handle.current_level(), LevelFilter::DEBUG);

        service
            .update_settings(SettingsPatch {
                debug_logging: Some(false),
                ..Default::default()
            })
            .await
            .expect("update settings");
        tokio::task::yield_now().await;
        assert_eq!(handle.current_level(), LevelFilter::INFO);
    }
}
