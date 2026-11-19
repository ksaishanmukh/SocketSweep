//! The GUI's commands, each declared once. The signature and the doc comment
//! of every function here are what `src/bindings.ts` is generated from
//! (parameter names become camelCase keys). Each one only forwards to the
//! [`App`] method of the same name; the work lives in `socketsweep-host-core`.

use std::sync::Arc;

use socketsweep_gui_contract::{
    CommandError, Connected, CopyResult, Crumb, DeletedMany, Device, ErrorCode, NodeRef, Overview,
    Row, ScanStarted, SearchRequest, SizeMetric, TreemapNode, View, ViewRequest,
};
use socketsweep_host_core::App;
use tauri::State;

type Host<'a> = State<'a, Arc<App>>;

/// Runs a host call on the blocking pool, so the window thread never waits on
/// the phone.
async fn blocking<T, F>(app: Host<'_>, call: F) -> Result<T, CommandError>
where
    T: Send + 'static,
    F: FnOnce(&App) -> Result<T, CommandError> + Send + 'static,
{
    let app = Arc::clone(app.inner());
    tauri::async_runtime::spawn_blocking(move || call(&app))
        .await
        .map_err(|e| CommandError::new(ErrorCode::Daemon, format!("Internal error: {e}")))?
}

/// Every attached phone, including ones that are not usable yet, so the
/// GUI can tell "nothing plugged in" from "tap Allow on the phone".
#[tauri::command]
#[specta::specta]
pub(crate) async fn list_devices(app: Host<'_>) -> Result<Vec<Device>, CommandError> {
    blocking(app, App::list_devices).await
}

/// Start a session on a phone, replacing any current one: push and start
/// the daemon and complete the handshake. `serial` defaults to the only
/// usable phone, `root` to `/sdcard`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn connect(
    app: Host<'_>,
    serial: Option<String>,
    root: Option<String>,
) -> Result<Connected, CommandError> {
    blocking(app, move |a| a.connect(serial, root)).await
}

/// End the session: cancel anything running, stop the daemon and forget
/// the tree.
#[tauri::command]
#[specta::specta]
pub(crate) async fn disconnect(app: Host<'_>) -> Result<(), CommandError> {
    blocking(app, App::disconnect).await
}

/// Start scanning `root` (default: the root given to `connect`), replacing
/// any previous tree. Resolves as soon as the phone has started, not when
/// it finishes: progress, the skeleton and the end arrive as
/// `scan-progress` events.
#[tauri::command]
#[specta::specta]
pub(crate) async fn start_scan(
    app: Host<'_>,
    root: Option<String>,
) -> Result<ScanStarted, CommandError> {
    blocking(app, move |a| a.start_scan(root)).await
}

/// Stop the running scan. Its last `scan-progress` says `cancelled`; the
/// tree so far stays queryable.
#[tauri::command]
#[specta::specta]
pub(crate) async fn cancel_scan(app: Host<'_>) -> Result<(), CommandError> {
    blocking(app, App::cancel_scan).await
}

/// The user opened folder `request.node` (by click, breadcrumb, treemap or
/// "go to"); a file means its folder. The phone measures it first, and
/// `scan-progress` carries this folder's view, sized by `by` (default
/// disk), in order `sort` (default largest first), narrowed by `filter`
/// (default none) and capped at `limit` rows (default 500). Call it again
/// for the same folder to change those; `getView` does not. No node means
/// the root. Hover and scrolling are not focus.
#[tauri::command]
#[specta::specta]
pub(crate) async fn focus(app: Host<'_>, request: ViewRequest) -> Result<(), CommandError> {
    blocking(app, move |a| a.focus(request)).await
}

/// A folder's rows, sized by `by` (default disk), in order `sort`
/// (default largest first), narrowed by `filter` (default none), at most
/// `limit` (default 500). `node` defaults to the root.
#[tauri::command]
#[specta::specta]
pub(crate) async fn get_view(app: Host<'_>, request: ViewRequest) -> Result<View, CommandError> {
    blocking(app, move |a| a.get_view(request)).await
}

/// The path from the root to `node`, root first.
#[tauri::command]
#[specta::specta]
pub(crate) async fn get_breadcrumbs(
    app: Host<'_>,
    node: NodeRef,
) -> Result<Vec<Crumb>, CommandError> {
    blocking(app, move |a| a.get_breadcrumbs(node)).await
}

/// `node` (default the root) and its descendants down to `depth` levels
/// (default 2), largest first by `by`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn get_treemap(
    app: Host<'_>,
    node: Option<NodeRef>,
    depth: Option<u32>,
    by: Option<SizeMetric>,
) -> Result<TreemapNode, CommandError> {
    blocking(app, move |a| a.get_treemap(node, depth, by)).await
}

/// Files and folders whose name contains `request.query`,
/// case-insensitively, and that pass `filter` (default none): inside folder
/// `within`, or anywhere when there is none. At most `limit` results
/// (default 200), in order `sort` (default largest first by `by`, default
/// disk). An empty query with no filter finds nothing.
#[tauri::command]
#[specta::specta]
pub(crate) async fn search(
    app: Host<'_>,
    request: SearchRequest,
) -> Result<Vec<Row>, CommandError> {
    blocking(app, move |a| a.search(request)).await
}

/// The summary of the whole scan so far, sized by `by` (default disk).
#[tauri::command]
#[specta::specta]
pub(crate) async fn overview(
    app: Host<'_>,
    by: Option<SizeMetric>,
) -> Result<Overview, CommandError> {
    blocking(app, move |a| a.overview(by)).await
}

/// The launcher icon of package `app` as a base64 PNG, once the phone has
/// reported it; `null` until then, or if it has none. Rows say whether
/// one exists with `hasIcon`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn app_icon(host: Host<'_>, app: String) -> Result<Option<String>, CommandError> {
    blocking(host, move |a| a.app_icon(app)).await
}

/// Delete nodes and everything below them from the phone, one after
/// another. Allowed during a scan, which skips the deleted part. Refused
/// as a whole, before anything is deleted, if any node is stale, already
/// deleted or the scan root. A node inside another one listed goes with
/// it. One node the phone will not delete does not stop the others; it is
/// reported in `failures`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn delete_many(
    app: Host<'_>,
    nodes: Vec<NodeRef>,
) -> Result<DeletedMany, CommandError> {
    blocking(app, move |a| a.delete_many(nodes)).await
}

/// Copy nodes (files, or folders and everything in them) to this computer
/// into `destination` (default: the desktop) as one copy, with one
/// `copy-progress` stream, resolving when the copy is complete. A node
/// inside another one listed goes with it. Refused as a whole for a stale
/// node.
#[tauri::command]
#[specta::specta]
pub(crate) async fn copy_many(
    app: Host<'_>,
    nodes: Vec<NodeRef>,
    destination: Option<String>,
) -> Result<CopyResult, CommandError> {
    blocking(app, move |a| a.copy_many(nodes, destination)).await
}

/// Stop the running copy. Files already copied stay.
#[tauri::command]
#[specta::specta]
pub(crate) async fn cancel_copy(app: Host<'_>) -> Result<(), CommandError> {
    blocking(app, App::cancel_copy).await
}

/// A plain-text report for bug reports: app version, OS, the connected
/// phone's model, Android API level and daemon build, and the last 200
/// log lines. `includePrivate` adds the phone's serial and the scan root
/// and leaves file paths in the log; without it they are masked.
#[tauri::command]
#[specta::specta]
pub(crate) async fn diagnostics(
    app: Host<'_>,
    include_private: bool,
) -> Result<String, CommandError> {
    blocking(app, move |a| a.diagnostics(include_private)).await
}

/// Write one line of the GUI's activity log to the log file.
#[tauri::command]
#[specta::specta]
pub(crate) async fn log_ui(app: Host<'_>, line: String) -> Result<(), CommandError> {
    blocking(app, move |a| a.log_ui(&line)).await
}
