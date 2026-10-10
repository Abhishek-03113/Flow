//! OS integrations driven by engine state: launch at login, the tray
//! icon's visibility and pairing-request notifications.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use flow_core::pairing::IncomingPairingRequest;
use flow_core::settings::FlowSettings;
use flow_daemon::ipc::events::StateEvent;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt as _;

use crate::bridge::StateObserver;
use crate::shell;
use crate::tray::TRAY_ID;

/// Whether the tray icon is shown. Without one, closing the window quits
/// (there would be no way back to a hidden app).
static TRAY_VISIBLE: AtomicBool = AtomicBool::new(true);

pub fn tray_is_visible() -> bool {
    TRAY_VISIBLE.load(Ordering::Relaxed)
}

/// Makes `launch_at_login` and `show_tray_icon` real.
pub struct SettingsSync;

impl StateObserver for SettingsSync {
    fn on_event(&self, app: &AppHandle, event: &StateEvent) {
        if event.name != "settings_changed" {
            return;
        }
        let Ok(settings) = serde_json::from_value::<FlowSettings>(event.payload.clone()) else {
            return;
        };
        sync_autostart(app, settings.launch_at_login);
        sync_tray(app, settings.show_tray_icon);
    }
}

fn sync_autostart(app: &AppHandle, wanted: bool) {
    let launcher = app.autolaunch();
    match launcher.is_enabled() {
        Ok(current) if current == wanted => {}
        _ => {
            let result = if wanted {
                launcher.enable()
            } else {
                launcher.disable()
            };
            if let Err(err) = result {
                tracing::warn!("could not update launch at login: {err}");
            }
        }
    }
}

fn sync_tray(app: &AppHandle, visible: bool) {
    TRAY_VISIBLE.store(visible, Ordering::Relaxed);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(visible);
    }
    if !visible {
        // Keep the app reachable.
        shell::show_main(app);
    }
}

/// Tells the user about a pairing request while the window is hidden.
/// Desktop notifications have no click callback, so the tray menu also
/// carries a "Review pairing request" item that opens the window; the
/// request sheet itself appears from engine state.
pub struct PairingNotifier {
    last_notified: Mutex<Option<String>>,
}

impl PairingNotifier {
    pub fn new() -> Self {
        Self {
            last_notified: Mutex::new(None),
        }
    }
}

impl StateObserver for PairingNotifier {
    fn on_event(&self, app: &AppHandle, event: &StateEvent) {
        if event.name != "incoming_pairing_request_changed" {
            return;
        }
        let request: Option<IncomingPairingRequest> =
            serde_json::from_value(event.payload.clone()).unwrap_or(None);
        let mut last = self.last_notified.lock().unwrap_or_else(|e| e.into_inner());
        let Some(request) = request else {
            *last = None;
            return;
        };
        if last.as_deref() == Some(request.request_id.as_str()) || window_is_in_front(app) {
            return;
        }
        *last = Some(request.request_id.clone());
        let shown = app
            .notification()
            .builder()
            .title("Pairing request")
            .body(format!(
                "{} wants to share its keyboard and mouse. Open Flow to accept or decline.",
                request.device_name
            ))
            .show();
        if let Err(err) = shown {
            tracing::warn!("could not show the pairing notification: {err}");
        }
    }
}

fn window_is_in_front(app: &AppHandle) -> bool {
    app.get_webview_window(shell::MAIN_WINDOW)
        .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
}
