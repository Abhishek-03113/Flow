//! Native tray skeleton: Open Flow, a device-list placeholder, Quit.
//! Track E replaces the placeholder with live devices and the `/tray` popover.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, Manager};

pub fn install(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Flow", true, None::<&str>)?;
    let devices = MenuItem::with_id(app, "devices", "No computers yet", false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Flow", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &PredefinedMenuItem::separator(app)?,
            &devices,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("flow")
        .tooltip("Flow")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
