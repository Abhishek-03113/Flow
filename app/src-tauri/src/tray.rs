//! Native tray (E2 replaces this skeleton with the live menu).

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::App;

use crate::shell;

pub fn install(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Flow", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Flow", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?, &quit])?;

    let mut builder = TrayIconBuilder::with_id("flow")
        .tooltip("Flow")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => shell::show_main(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
