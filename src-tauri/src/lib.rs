//! The desktop shell: a thin layer that builds the host, exposes each
//! contract command to the window and forwards the host's events. All logic
//! lives in `socketsweep-host-core`.

mod commands;
mod logging;

use std::path::PathBuf;
use std::sync::Arc;

use socketsweep_gui_contract::{AppsUpdated, ConnectionLost, CopyProgress, ScanProgress};
use socketsweep_host_core::{App, AppConfig, DaemonBinaries, HostEvent};
use socketsweep_transport_adb::AdbTransport;
use tauri::{Manager, Wry};
use tauri_specta::{collect_commands, collect_events, Builder, ErrorHandlingMode, Event};

/// Every command and event the window can use, in one place: the invoke
/// handler, the event registry and the generated `src/bindings.ts` all come
/// from this.
pub fn specta_builder() -> Builder<Wry> {
    Builder::<Wry>::new()
        // A failed command rejects with its `CommandError`, as the GUI expects.
        .error_handling(ErrorHandlingMode::Throw)
        // One TypeScript type per Rust type, not a request and a response form.
        .disable_serde_phases()
        .commands(collect_commands![
            commands::list_devices,
            commands::connect,
            commands::disconnect,
            commands::start_scan,
            commands::cancel_scan,
            commands::focus,
            commands::get_view,
            commands::get_breadcrumbs,
            commands::get_treemap,
            commands::search,
            commands::overview,
            commands::app_icon,
            commands::delete_many,
            commands::copy_many,
            commands::cancel_copy,
            commands::diagnostics,
            commands::log_ui,
        ])
        .events(collect_events![
            ScanProgress,
            CopyProgress,
            ConnectionLost,
            AppsUpdated,
        ])
}

/// Emit a host event to the window under its contract name.
fn forward(handle: &tauri::AppHandle, event: HostEvent) {
    let (name, sent) = match &event {
        HostEvent::ScanProgress(p) => (ScanProgress::NAME, p.emit(handle)),
        HostEvent::CopyProgress(p) => (CopyProgress::NAME, p.emit(handle)),
        HostEvent::ConnectionLost(p) => (ConnectionLost::NAME, p.emit(handle)),
        HostEvent::AppsUpdated(p) => (AppsUpdated::NAME, p.emit(handle)),
    };
    if let Err(e) = sent {
        tracing::warn!("could not emit {name}: {e}");
    }
}

/// A bundled resource under `bin/`.
fn bundled(handle: &tauri::AppHandle, file: &str) -> Result<PathBuf, tauri::Error> {
    Ok(handle.path().resource_dir()?.join("bin").join(file))
}

/// Keeps the log writer's thread flushing until the app exits.
#[allow(dead_code)]
struct LogGuard(tracing_appender::non_blocking::WorkerGuard);

/// Starts file logging; the app runs without it if the folder is unusable.
fn start_logging(handle: &tauri::AppHandle) -> Option<PathBuf> {
    let dir = handle.path().app_log_dir().ok()?;
    match logging::init(&dir) {
        Ok(guard) => {
            handle.manage(LogGuard(guard));
            Some(dir)
        }
        Err(e) => {
            // No subscriber exists yet to take this.
            eprintln!("could not open the log folder {}: {e}", dir.display());
            None
        }
    }
}

fn build_app(handle: &tauri::AppHandle) -> Result<App, Box<dyn std::error::Error>> {
    let adb = bundled(handle, if cfg!(windows) { "adb.exe" } else { "adb" })?;

    let paths = handle.path();
    let config = AppConfig {
        daemon: DaemonBinaries {
            arm64: Some(bundled(handle, "daemon-arm64.gz")?),
            x86_64: Some(bundled(handle, "daemon-x86_64.gz")?),
            helper: Some(bundled(handle, "socketsweep-helper.dex")?),
        },
        log_dir: start_logging(handle),
        history_file: paths.app_data_dir().ok().map(|d| d.join("history.json")),
        default_destination: paths.desktop_dir().or_else(|_| paths.download_dir()).ok(),
        ..AppConfig::default()
    };

    let emitter = handle.clone();
    // adb starts on the first command that needs a phone, so a failure to
    // start it reaches the window as an error instead of stopping it opening.
    Ok(App::lazy(
        move || AdbTransport::connect(&adb),
        config,
        move |event| forward(&emitter, event),
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let specta = specta_builder();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(specta.invoke_handler())
        .setup(move |tauri_app| {
            specta.mount_events(tauri_app);
            let host = Arc::new(build_app(tauri_app.handle())?);
            tauri_app.manage(host);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the SocketSweep shell")
        .run(|handle, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(host) = handle.try_state::<Arc<App>>() {
                    host.shutdown();
                }
            }
        });
}
