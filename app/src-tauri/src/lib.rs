//! Flow desktop app (Tauri 2): hosts the engine in-process and bridges it
//! to the webview, the tray and the OS. All product logic lives in the
//! engine (`flow-daemon`); this crate only adapts it to a window.

mod bridge;
mod shell;
mod tray;

use std::sync::Arc;

use flow_daemon::devmode::DevMode;
use flow_daemon::engine::Engine as EngineRuntime;
use flow_daemon::env_config::{self, Profile};
use flow_daemon::platform_backend::NativeBackend;
use tauri::{Manager, RunEvent, WindowEvent};

use bridge::{Engine, StateObserver, WebviewForwarder};

/// Starts the engine in-process: same configuration builder as the
/// headless `flow-daemon`, without a local socket.
fn start_engine() -> Result<flow_daemon::engine::EngineHandle, String> {
    let dev = DevMode::from_env();
    let logging = flow_daemon::logging::init(false, dev.trace);
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

    builder
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![bridge::flow_command])
        .on_window_event(|window, event| {
            // Closing the window hides it: Flow keeps running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == shell::MAIN_WINDOW {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let handle = start_engine()?;
            if let Some(addr) = handle.ipc_addr() {
                tracing::info!("developer WebSocket enabled on {addr}");
            }
            tracing::info!("engine started in-process");
            let observers: Vec<Box<dyn StateObserver>> = vec![Box::new(WebviewForwarder)];
            let engine = Arc::new(Engine::new(handle, observers));
            app.manage(Arc::clone(&engine));
            tray::install(app)?;
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
