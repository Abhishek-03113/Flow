//! Window and process lifecycle: the app lives in the tray, so closing the
//! window only hides it; the engine runs until Quit.

use serde_json::Value;
use tauri::{AppHandle, Manager, WebviewWindow};

pub const MAIN_WINDOW: &str = "main";
/// Passed by the login item so Flow starts in the tray, not in your face.
pub const MINIMIZED_FLAG: &str = "--minimized";

/// Shows the main window at startup, except when launched at login.
pub fn show_main_unless_minimized(app: &AppHandle) {
    if !std::env::args().any(|a| a == MINIMIZED_FLAG) {
        show_main(app);
    }
}

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

/// Commands only the shell can answer (they never reach the engine).
/// `None` means "not a shell command".
pub fn app_command(
    app: &AppHandle,
    command: &str,
    payload: &Value,
) -> Option<Result<Value, flow_core::ipc::ErrorPayload>> {
    use tauri::Emitter;
    match command {
        // The tray popover asks for the full window, optionally with a
        // sheet (`pairing`, `permission`) or a route open.
        "open_main_window" => {
            show_main(app);
            if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                let _ = window.emit("flow://navigate", payload);
            }
            if let Some(popover) = app.get_webview_window(crate::tray::POPOVER) {
                let _ = popover.hide();
            }
            Some(Ok(Value::Null))
        }
        _ => None,
    }
}
