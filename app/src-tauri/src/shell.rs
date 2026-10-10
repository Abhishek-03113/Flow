//! Window and process lifecycle: the app lives in the tray, so closing the
//! window only hides it; the engine runs until Quit.

use serde_json::Value;
use tauri::{AppHandle, Manager, WebviewWindow};

pub const MAIN_WINDOW: &str = "main";

/// Shows and focuses the main window.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        reveal(&window);
    }
}

fn reveal(window: &WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Side effects only the app shell can perform, driven by the engine's
/// answer to a command (the engine itself stays headless-safe: it just
/// returns the path or URL).
pub fn after_command(app: &AppHandle, command: &str, result: &Value) {
    let target = match command {
        "reveal_config_dir" => result.get("path"),
        "open_permission_settings" => result.get("url"),
        _ => return,
    };
    if let Some(target) = target.and_then(Value::as_str) {
        use tauri_plugin_opener::OpenerExt;
        let opened = if target.contains("://") || target.starts_with("ms-settings:") {
            app.opener().open_url(target, None::<&str>)
        } else {
            app.opener().open_path(target, None::<&str>)
        };
        if let Err(err) = opened {
            tracing::warn!("could not open {target}: {err}");
        }
    }
}
