//! Flow desktop app (Tauri 2): hosts the engine in-process and bridges it
//! to the webview, the tray and the OS. All product logic lives in the
//! engine (`flow-daemon`); this crate only adapts it to a window.

mod bridge;
mod integrations;
mod shell;
mod tray;
mod tray_model;

use std::sync::Arc;

use flow_daemon::devmode::DevMode;
use flow_daemon::engine::Engine as EngineRuntime;
use flow_daemon::env_config::{self, Profile};
use flow_daemon::platform_backend::NativeBackend;
use tauri::{Manager, RunEvent, WindowEvent};

use bridge::{Engine, StateObserver, WebviewForwarder};
use integrations::{PairingNotifier, SettingsSync};

/// Starts the engine in-process: same configuration builder as the
/// headless `flow-daemon`, without a local socket.
fn start_engine() -> Result<flow_daemon::engine::EngineHandle, String> {
    let dev = DevMode::from_env();
    // Test hooks and insecure mode exist for the headless daemon, the
    // harness and debug builds; a release app never honours them.
    let dev = if cfg!(debug_assertions) {
        dev
    } else {
        dev.without_test_switches()
    };
    // The app has no console: keep a log file next to the engine's data
    // (`<data dir>/logs/flow.log`) so failures can be diagnosed afterwards.
    let log_dir = env_config::data_dir().ok().map(|dir| dir.join("logs"));
    let logging = flow_daemon::logging::init_with_dir(false, dev.trace, log_dir.as_deref());
    dev.announce();
    let config = env_config::engine_config(
        Profile::Embedded,
        Arc::new(NativeBackend),
        &dev,
        Some(logging),
    )
    .map_err(|err| err.to_string())?;
    tauri::async_runtime::block_on(EngineRuntime::start(config)).map_err(|err| err.to_string())
}

pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        // A second launch just brings up the running app.
        shell::show_main(app);
    }));

    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_autostart::init(
        tauri_plugin_autostart::MacosLauncher::LaunchAgent,
        Some(vec![shell::MINIMIZED_FLAG]),
    ));

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![bridge::flow_command])
        .on_window_event(|window, event| match event {
            // Closing the window hides it: Flow keeps running in the tray
            // (unless the tray icon is turned off, then closing quits).
            WindowEvent::CloseRequested { api, .. }
                if window.label() == shell::MAIN_WINDOW && integrations::tray_is_visible() =>
            {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Focused(focused) => tray::hide_popover_on_blur(window, *focused),
            _ => {}
        })
        .setup(|app| {
            let handle = start_engine()?;
            if let Some(addr) = handle.ipc_addr() {
                tracing::info!("developer WebSocket enabled on {addr}");
            }
            tracing::info!("engine started in-process");
            let observers: Vec<Box<dyn StateObserver>> = vec![
                Box::new(WebviewForwarder),
                Box::new(tray::TrayObserver::new()),
                Box::new(SettingsSync),
                Box::new(PairingNotifier::new()),
            ];
            let engine = Arc::new(Engine::new(handle, observers));
            app.manage(Arc::clone(&engine));
            tray::install(app)?;
            shell::show_main_unless_minimized(app.handle());
            engine.start_pump(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Flow")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                if let Some(engine) = bridge::engine_of(app) {
                    engine.shutdown();
                }
            }
        });
}
