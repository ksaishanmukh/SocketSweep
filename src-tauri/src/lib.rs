//! The desktop shell: a thin layer that builds the host, exposes each
//! contract command to the window and forwards the host's events.
//!
//! All logic lives in `socketsweep-host-core`. This file only wires paths,
//! commands and events. See `docs/architecture.md`, section 1.

use std::path::PathBuf;
use std::sync::Arc;

use socketsweep_gui_contract::*;
use socketsweep_host_core::{App, AppConfig, DaemonBinaries, HostEvent};
use socketsweep_transport_adb::AdbTransport;
use tauri::{Emitter, Manager, State};

/// Declares one `#[tauri::command]` per contract method, with the contract's
/// name and parameter names (Tauri maps the camelCase keys the generated
/// bindings send onto these snake_case parameters). Each runs the blocking
/// host call on the blocking pool, so the window thread never waits on the
/// phone.
///
/// The list below must match [`Commands`] exactly: a missing method fails to
/// compile through the trait, and a test compares [`REGISTERED`] with
/// [`COMMAND_NAMES`].
macro_rules! shell_commands {
    ($(fn $name:ident($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty;)*) => {
        $(
            #[tauri::command]
            async fn $name(
                app: State<'_, Arc<App>>,
                $($arg: $ty),*
            ) -> Result<$ret, CommandError> {
                let app = Arc::clone(app.inner());
                tauri::async_runtime::spawn_blocking(move || Commands::$name(&*app, $($arg),*))
                    .await
                    .map_err(|e| {
                        CommandError::new(ErrorCode::Daemon, format!("Internal error: {e}"))
                    })?
            }
        )*

        /// The commands registered with Tauri, in contract order.
        #[cfg(test)]
        const REGISTERED: &[&str] = &[$(stringify!($name)),*];

        fn invoke_handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
            tauri::generate_handler![$($name),*]
        }
    };
}

shell_commands! {
    fn list_devices() -> Vec<Device>;
    fn connect(serial: Option<String>, root: Option<String>) -> Connected;
    fn disconnect() -> ();
    fn start_scan(root: Option<String>) -> ScanStarted;
    fn cancel_scan() -> ();
    fn focus(node: Option<NodeRef>, limit: Option<u32>, by: Option<SizeMetric>) -> ();
    fn get_view(node: Option<NodeRef>, limit: Option<u32>, by: Option<SizeMetric>) -> View;
    fn get_breadcrumbs(node: NodeRef) -> Vec<Crumb>;
    fn get_treemap(node: Option<NodeRef>, depth: Option<u32>, by: Option<SizeMetric>) -> TreemapNode;
    fn largest_files(limit: Option<u32>, by: Option<SizeMetric>) -> Vec<Row>;
    fn type_breakdown(by: Option<SizeMetric>) -> Vec<TypeGroup>;
    fn app_breakdown(limit: Option<u32>, by: Option<SizeMetric>) -> Vec<AppUsage>;
    fn search(query: String, limit: Option<u32>, by: Option<SizeMetric>) -> Vec<Row>;
    fn delete(node: NodeRef) -> Deleted;
    fn copy_to_desktop(node: NodeRef, destination: Option<String>) -> CopyResult;
    fn cancel_copy() -> ();
}

/// Emit a host event to the window under its contract name.
fn forward(handle: &tauri::AppHandle, event: HostEvent) {
    let name = event.name();
    let sent = match &event {
        HostEvent::ScanProgress(p) => handle.emit(name, p),
        HostEvent::CopyProgress(p) => handle.emit(name, p),
        HostEvent::ConnectionLost(p) => handle.emit(name, p),
    };
    if let Err(e) = sent {
        eprintln!("could not emit {name}: {e}");
    }
}

/// A bundled resource under `bin/`.
fn bundled(handle: &tauri::AppHandle, file: &str) -> Result<PathBuf, tauri::Error> {
    Ok(handle.path().resource_dir()?.join("bin").join(file))
}

fn build_app(handle: &tauri::AppHandle) -> Result<App, Box<dyn std::error::Error>> {
    let adb = bundled(handle, if cfg!(windows) { "adb.exe" } else { "adb" })?;

    let paths = handle.path();
    let config = AppConfig {
        daemon: DaemonBinaries {
            arm64: Some(bundled(handle, "daemon-arm64")?),
            x86_64: Some(bundled(handle, "daemon-x86_64")?),
        },
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
    tauri::Builder::default()
        .setup(|tauri_app| {
            let host = Arc::new(build_app(tauri_app.handle())?);
            tauri_app.manage(host);
            Ok(())
        })
        .invoke_handler(invoke_handler())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_exactly_the_contract_commands() {
        assert_eq!(REGISTERED, COMMAND_NAMES);
    }
}
