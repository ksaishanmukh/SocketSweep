//! The commands the GUI invokes.
//!
//! One list, written once in [`commands!`], produces both sides of the
//! contract:
//!
//! - [`Commands`], the trait the host implements, so a command the GUI can
//!   call but the host does not serve fails to compile;
//! - [`command_defs`], the same signatures as data, from which
//!   [`bindings`](crate::bindings) generates the TypeScript wrappers.
//!
//! The Tauri shell exposes each method as a `#[tauri::command]` of the same
//! name with the same parameters. Tauri passes arguments as an object keyed
//! by the parameter names in camelCase, which is what the generated wrappers
//! send.

use specta::datatype::DataType;
use specta::Types;

use crate::*;

/// One command's signature, for generating bindings.
#[derive(Debug, Clone)]
pub struct CommandDef {
    /// The Rust and Tauri name, snake_case.
    pub name: &'static str,
    pub docs: &'static str,
    /// Parameter names (snake_case) and types.
    pub args: Vec<(&'static str, DataType)>,
    /// `None` for a command that returns nothing.
    pub ret: Option<DataType>,
}

/// `Some(definition)`, or `None` for `()`.
fn ret_def<T: specta::Type + 'static>(types: &mut Types) -> Option<DataType> {
    (std::any::TypeId::of::<T>() != std::any::TypeId::of::<()>()).then(|| T::definition(types))
}

macro_rules! commands {
    ($(
        $(#[doc = $doc:literal])*
        fn $name:ident($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty;
    )*) => {
        /// Everything the GUI can ask of the host. The shell exposes each
        /// method as a `#[tauri::command]`.
        ///
        /// Every method may block on the phone; the shell runs them off the
        /// thread that drives the window.
        pub trait Commands: Send + Sync {
            $(
                $(#[doc = $doc])*
                fn $name(&self, $($arg: $ty),*) -> Result<$ret, CommandError>;
            )*
        }

        /// Every command's signature, registering the types they use in
        /// `types`.
        pub fn command_defs(types: &mut Types) -> Vec<CommandDef> {
            vec![$(
                CommandDef {
                    name: stringify!($name),
                    docs: concat!($($doc, "\n"),*),
                    args: vec![$((stringify!($arg), <$ty as specta::Type>::definition(types))),*],
                    ret: ret_def::<$ret>(types),
                },
            )*]
        }

        /// Every command's Tauri name, for registering them in the shell.
        pub const COMMAND_NAMES: &[&str] = &[$(stringify!($name)),*];
    };
}

commands! {
    /// Every attached phone, including ones that are not usable yet, so the
    /// GUI can tell "nothing plugged in" from "tap Allow on the phone".
    fn list_devices() -> Vec<Device>;

    /// Start a session on a phone, replacing any current one: push and start
    /// the daemon and complete the handshake. `serial` defaults to the only
    /// usable phone, `root` to `/sdcard`.
    fn connect(serial: Option<String>, root: Option<String>) -> Connected;

    /// End the session: cancel anything running, stop the daemon and forget
    /// the tree.
    fn disconnect() -> ();

    /// Start scanning `root` (default: the root given to `connect`), replacing
    /// any previous tree. Resolves as soon as the phone has started, not when
    /// it finishes: progress, the skeleton and the end arrive as
    /// `scan-progress` events.
    fn start_scan(root: Option<String>) -> ScanStarted;

    /// Stop the running scan. Its last `scan-progress` says `cancelled`; the
    /// tree so far stays queryable.
    fn cancel_scan() -> ();

    /// The user opened folder `node` (by click, breadcrumb, treemap or "go
    /// to"); a file means its folder. The phone measures it first, and
    /// `scan-progress` carries this folder's view, sized by `by` (default
    /// disk), in order `sort` (default largest first), narrowed by `filter`
    /// (default none) and capped at `limit` rows (default 500). Call it again
    /// for the same folder to change those; `getView` does not. `null` means
    /// the root. Hover and scrolling are not focus.
    fn focus(
        node: Option<NodeRef>,
        limit: Option<u32>,
        by: Option<SizeMetric>,
        sort: Option<ViewSort>,
        filter: Option<ViewFilter>,
    ) -> ();

    /// A folder's rows, sized by `by` (default disk), in order `sort`
    /// (default largest first), narrowed by `filter` (default none), at most
    /// `limit` (default 500). `node` defaults to the root.
    fn get_view(
        node: Option<NodeRef>,
        limit: Option<u32>,
        by: Option<SizeMetric>,
        sort: Option<ViewSort>,
        filter: Option<ViewFilter>,
    ) -> View;

    /// The path from the root to `node`, root first.
    fn get_breadcrumbs(node: NodeRef) -> Vec<Crumb>;

    /// `node` (default the root) and its descendants down to `depth` levels
    /// (default 2), largest first by `by`.
    fn get_treemap(node: Option<NodeRef>, depth: Option<u32>, by: Option<SizeMetric>) -> TreemapNode;

    /// Files and folders whose name contains `query`, case-insensitively, and
    /// that pass `filter` (default none): inside folder `within`, or anywhere
    /// when it is `null`. At most `limit` results (default 200), in order
    /// `sort` (default largest first by `by`, default disk). An empty query
    /// with no filter finds nothing.
    fn search(
        query: String,
        limit: Option<u32>,
        by: Option<SizeMetric>,
        filter: Option<SearchFilter>,
        sort: Option<ViewSort>,
        within: Option<NodeRef>,
    ) -> Vec<Row>;

    /// The summary of the whole scan so far, sized by `by` (default disk).
    fn overview(by: Option<SizeMetric>) -> Overview;

    /// The launcher icon of package `app` as a base64 PNG, once the phone has
    /// reported it; `null` until then, or if it has none. Rows say whether
    /// one exists with `hasIcon`.
    fn app_icon(app: String) -> Option<String>;

    /// Delete nodes and everything below them from the phone, one after
    /// another. Allowed during a scan, which skips the deleted part. Refused
    /// as a whole, before anything is deleted, if any node is stale, already
    /// deleted or the scan root. A node inside another one listed goes with
    /// it. One node the phone will not delete does not stop the others; it is
    /// reported in `failures`.
    fn delete_many(nodes: Vec<NodeRef>) -> DeletedMany;

    /// Copy nodes (files, or folders and everything in them) to this computer
    /// into `destination` (default: the desktop) as one copy, with one
    /// `copy-progress` stream, resolving when the copy is complete. A node
    /// inside another one listed goes with it. Refused as a whole for a stale
    /// node.
    fn copy_many(nodes: Vec<NodeRef>, destination: Option<String>) -> CopyResult;

    /// Stop the running copy. Files already copied stay.
    fn cancel_copy() -> ();
}
