//! The native tray: a menu rebuilt from engine state, plus (Windows and
//! macOS) a small popover window toggled by left-clicking the icon.
//!
//! Linux uses the menu only: the appindicator backend does not report icon
//! clicks, so there is nothing to toggle a popover with.

use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

use crate::bridge::{engine_of, StateObserver};
use crate::shell;
use crate::tray_model::{Entry, TrayModel, ID_HOME, ID_OPEN, ID_QUIT, ID_REQUEST, SWITCH_PREFIX};
use flow_daemon::ipc::events::StateEvent;

pub const TRAY_ID: &str = "flow";
pub const POPOVER: &str = "tray";
const POPOVER_SIZE: (f64, f64) = (360.0, 480.0);

fn build_menu(app: &AppHandle, entries: &[Entry]) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    for entry in entries {
        match entry {
            Entry::Separator => menu.append(&PredefinedMenuItem::separator(app)?)?,
            Entry::Item {
                id,
                label,
                enabled,
                checked: None,
            } => menu.append(&MenuItem::with_id(app, id, label, *enabled, None::<&str>)?)?,
            Entry::Item {
                id,
                label,
                enabled,
                checked: Some(on),
            } => menu.append(&CheckMenuItem::with_id(
                app,
                id,
                label,
                *enabled,
                *on,
                None::<&str>,
            )?)?,
        }
    }
    Ok(menu)
}

/// Keeps the tray menu in step with the engine.
pub struct TrayObserver {
    model: Mutex<TrayModel>,
}

impl TrayObserver {
    pub fn new() -> Self {
        Self {
            model: Mutex::new(TrayModel::default()),
        }
    }
}

impl StateObserver for TrayObserver {
    fn on_event(&self, app: &AppHandle, event: &StateEvent) {
        let (changed, entries, status) = {
            let mut model = self.model.lock().unwrap_or_else(|e| e.into_inner());
            let changed = model.apply(event);
            (changed, model.entries(), model.status_line())
        };
        if !changed {
            return;
        }
        let Some(tray) = app.tray_by_id(TRAY_ID) else {
            return;
        };
        match build_menu(app, &entries) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(err) => tracing::warn!("could not rebuild the tray menu: {err}"),
        }
        let _ = tray.set_tooltip(Some(format!("Flow \u{2014} {status}")));
    }
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        ID_OPEN | ID_REQUEST => shell::show_main(app),
        ID_QUIT => app.exit(0),
        ID_HOME => run_switch(app, "local"),
        other => {
            if let Some(device) = other.strip_prefix(SWITCH_PREFIX) {
                run_switch(app, device);
            }
        }
    }
}

fn run_switch(app: &AppHandle, target: &str) {
    let Some(engine) = engine_of(app) else { return };
    let target = target.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(err) = crate::bridge::execute(
            engine.service(),
            "switch_to",
            serde_json::json!({ "target": target }),
        )
        .await
        {
            tracing::warn!("tray switch failed: {}", err.message);
        }
    });
}

pub fn install(app: &tauri::App) -> tauri::Result<()> {
    let handle = app.handle().clone();
    let initial = TrayModel::default().entries();
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Flow")
        .menu(&build_menu(&handle, &initial)?)
        .on_menu_event(|app, event| on_menu(app, event.id.as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                toggle_popover(tray.app_handle(), position);
            }
        });
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        // Left-click opens the popover; the menu stays on right-click.
        builder = builder.show_menu_on_left_click(false);
    }
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Shows the popover next to the tray icon, or hides it if it is open.
fn toggle_popover(app: &AppHandle, click: PhysicalPosition<f64>) {
    let window = match app.get_webview_window(POPOVER) {
        Some(window) => window,
        None => match WebviewWindowBuilder::new(
            app,
            POPOVER,
            WebviewUrl::App("index.html#/tray".into()),
        )
        .title("Flow")
        .inner_size(POPOVER_SIZE.0, POPOVER_SIZE.1)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()
        {
            Ok(window) => window,
            Err(err) => {
                tracing::warn!("could not create the tray popover: {err}");
                return;
            }
        },
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    let (width, height) = (POPOVER_SIZE.0 * scale, POPOVER_SIZE.1 * scale);
    let screen_height = window
        .current_monitor()
        .ok()
        .flatten()
        .map_or(f64::MAX, |m| f64::from(m.size().height));
    // A tray at the bottom (Windows) opens upwards, at the top (macOS) downwards.
    let y = if click.y > screen_height / 2.0 {
        click.y - height - 8.0
    } else {
        click.y + 8.0
    };
    let x = (click.x - width / 2.0).max(0.0);
    let _ = window.set_position(PhysicalPosition::new(x, y.max(0.0)));
    let _ = window.show();
    let _ = window.set_focus();
}

/// Closes the popover when it loses focus, like a native menu.
pub fn hide_popover_on_blur(window: &tauri::Window, focused: bool) {
    if !focused && window.label() == POPOVER {
        let _ = window.hide();
    }
}
