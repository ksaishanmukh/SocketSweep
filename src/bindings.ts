// Generated from crates/gui-contract by
//   cargo run -p socketsweep-gui-contract --bin export-bindings
// Do not edit by hand: CI fails if this file differs from what the crate generates.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * Every host command. Each rejects with a `CommandError` when it fails.
 */
export const commands = {
  /**
   * Every attached phone, including ones that are not usable yet, so the
   * GUI can tell "nothing plugged in" from "tap Allow on the phone".
   */
  listDevices: (): Promise<Device[]> =>
    invoke<Device[]>("list_devices"),
  /**
   * Start a session on a phone, replacing any current one: push and start
   * the daemon and complete the handshake. `serial` defaults to the only
   * usable phone, `root` to `/sdcard`.
   */
  connect: (serial?: string | null, root?: string | null): Promise<Connected> =>
    invoke<Connected>("connect", { serial, root }),
  /**
   * End the session: cancel anything running, stop the daemon and forget
   * the tree.
   */
  disconnect: (): Promise<void> =>
    invoke<void>("disconnect"),
  /**
   * Start scanning `root` (default: the root given to `connect`), replacing
   * any previous tree. Resolves as soon as the phone has started, not when
   * it finishes: progress, the skeleton and the end arrive as
   * `scan-progress` events.
   */
  startScan: (root?: string | null): Promise<ScanStarted> =>
    invoke<ScanStarted>("start_scan", { root }),
  /**
   * Stop the running scan. Its last `scan-progress` says `cancelled`; the
   * tree so far stays queryable.
   */
  cancelScan: (): Promise<void> =>
    invoke<void>("cancel_scan"),
  /**
   * The user opened folder `node` (by click, breadcrumb, treemap or "go
   * to"); a file means its folder. The phone measures it first, and
   * `scan-progress` carries this folder's view, sorted by `by` (default
   * disk) and capped at `limit` rows (default 500). Call it again for the
   * same folder to change those; `getView` does not. `null` means the root.
   * Hover and scrolling are not focus.
   */
  focus: (node?: NodeRef | null, limit?: number | null, by?: SizeMetric | null): Promise<void> =>
    invoke<void>("focus", { node, limit, by }),
  /**
   * A folder's rows, largest first by `by` (default disk). `node`
   * defaults to the root, `limit` to 500.
   */
  getView: (node?: NodeRef | null, limit?: number | null, by?: SizeMetric | null): Promise<View> =>
    invoke<View>("get_view", { node, limit, by }),
  /**
   * The path from the root to `node`, root first.
   */
  getBreadcrumbs: (node: NodeRef): Promise<Crumb[]> =>
    invoke<Crumb[]>("get_breadcrumbs", { node }),
  /**
   * `node` (default the root) and its descendants down to `depth` levels
   * (default 2), largest first by `by`.
   */
  getTreemap: (node?: NodeRef | null, depth?: number | null, by?: SizeMetric | null): Promise<TreemapNode> =>
    invoke<TreemapNode>("get_treemap", { node, depth, by }),
  /**
   * The largest files anywhere in the tree (default 100).
   */
  largestFiles: (limit?: number | null, by?: SizeMetric | null): Promise<Row[]> =>
    invoke<Row[]>("largest_files", { limit, by }),
  /**
   * Total size per broad file category.
   */
  typeBreakdown: (by?: SizeMetric | null): Promise<TypeGroup[]> =>
    invoke<TypeGroup[]>("type_breakdown", { by }),
  /**
   * Storage per app package (default 50).
   */
  appBreakdown: (limit?: number | null, by?: SizeMetric | null): Promise<AppUsage[]> =>
    invoke<AppUsage[]>("app_breakdown", { limit, by }),
  /**
   * Files and folders whose name contains `query`, case-insensitively
   * (default 200 results), largest first by `by` (default disk).
   */
  search: (query: string, limit?: number | null, by?: SizeMetric | null): Promise<Row[]> =>
    invoke<Row[]>("search", { query, limit, by }),
  /**
   * Delete `node` and everything below it from the phone. Refused for a
   * stale node and for the scan root. Allowed during a scan, which skips
   * the deleted part.
   */
  delete: (node: NodeRef): Promise<Deleted> =>
    invoke<Deleted>("delete", { node }),
  /**
   * Copy `node` (a file, or a folder and everything in it) to this
   * computer, into `destination` (default: the desktop). Progress arrives
   * as `copy-progress` events; resolves when the copy is complete.
   */
  copyToDesktop: (node: NodeRef, destination?: string | null): Promise<CopyResult> =>
    invoke<CopyResult>("copy_to_desktop", { node, destination }),
  /**
   * Stop the running copy. Files already copied stay.
   */
  cancelCopy: (): Promise<void> =>
    invoke<void>("cancel_copy"),
};

/**
 * Every host event. Each resolves to a function that stops listening.
 */
export const events = {
  scanProgress: (handler: (payload: ScanProgress) => void): Promise<UnlistenFn> =>
    listen<ScanProgress>("scan-progress", (e) => handler(e.payload)),
  copyProgress: (handler: (payload: CopyProgress) => void): Promise<UnlistenFn> =>
    listen<CopyProgress>("copy-progress", (e) => handler(e.payload)),
  connectionLost: (handler: (payload: ConnectionLost) => void): Promise<UnlistenFn> =>
    listen<ConnectionLost>("connection-lost", (e) => handler(e.payload)),
};
/**  Storage owned by one app, summed across its data, obb and media folders. */
export type AppUsage = {
	/**
	 *  Android package name, e.g. `com.example.game`. Not a display label:
	 *  adb's shell has no access to app labels.
	 */
	package: string,
	size: Size,
	files: number,
	/**  The largest folder the package owns, for navigating to it. */
	node: NodeRef,
};

/**  How every command fails. */
export type CommandError = {
	code: ErrorCode,
	/**  For the user, already worded. */
	message: string,
};

/**  The answer to `connect`. */
export type Connected = {
	serial: string,
	model: string,
	/**  The root that scans default to, as the user knows it (`/sdcard`). */
	root: string,
	/**
	 *  Where `copyToDesktop` copies when no destination is given, as a path on
	 *  this computer (the desktop), so the GUI can name the action up front.
	 *  `null` when the host knows no default; a copy must then name one.
	 */
	copyDestination: string | null,
	/**
	 *  The last finished scan of this device and root, if it has been seen
	 *  before, for showing what changed.
	 */
	previous: ScanRecord | null,
};

/**
 *  Emitted as `connection-lost` when the phone stops answering or is
 *  unplugged. The session is over; nodes from it are stale.
 */
export type ConnectionLost = {
	serial: string,
	message: string,
};

/**  Emitted as `copy-progress` while `copyToDesktop` runs. */
export type CopyProgress = {
	/**  What is being copied. */
	node: NodeRef,
	destination: string,
	filesDone: number,
	filesTotal: number,
	bytesDone: number,
	bytesTotal: number,
	/**
	 *  False while the phone is still listing what to copy, so the totals
	 *  are still growing.
	 */
	totalsKnown: boolean,
	/**  The file being copied, relative to what was asked for. */
	current: string | null,
};

/**  The answer to `copyToDesktop`, once every file is copied. */
export type CopyResult = {
	/**  Where the copy is, on this computer. */
	destination: string,
	files: number,
	bytes: number,
	/**  Symbolic links and special files, which are not copied. */
	skipped: number,
	/**
	 *  Files whose Android name is not valid here, copied under a changed
	 *  name.
	 */
	renamed: number,
};

/**  One step of the path from the root to a node. */
export type Crumb = {
	node: NodeRef,
	name: string,
};

/**  The answer to `delete`, once the phone confirms. */
export type Deleted = {
	/**  Files and folders removed. */
	items: number,
	/**  The whole tree's totals afterwards. */
	totals: Totals,
	/**
	 *  The focused folder, refreshed; its nearest surviving ancestor if it
	 *  was inside what was deleted.
	 */
	view: View,
};

/**  An attached phone, usable or not. */
export type Device = {
	serial: string,
	/**  Human-readable model for the picker; falls back to the serial. */
	model: string,
	state: DeviceState,
	/**  True only when `state` is `ready`. */
	usable: boolean,
	/**  What to do about a device that cannot be used yet, when the host knows. */
	hint: string | null,
};

export type DeviceState = "ready" | 
/**  Waiting for the user to accept the debugging prompt on the phone. */
"unauthorized" | "offline" | 
/**  The computer will not let adb open it (Linux without a udev rule). */
"noPermission" | "other";

/**  What a node is. Symbolic links are never followed. */
export type EntryKind = "file" | "dir" | "symlink" | 
/**  Sockets, pipes, devices. */
"other";

export type ErrorCode = 
/**  No device is connected. */
"notConnected" | 
/**  Connected, but no scan has started. */
"noScan" | 
/**  The node is from an earlier session. The GUI refreshes. */
"stale" | 
/**  The node has since been deleted. */
"removed" | 
/**  Another operation that cannot overlap this one is running. */
"busy" | 
/**  adb or the phone refused or failed. */
"device" | 
/**  The daemon on the phone refused or failed. */
"daemon" | 
/**  The user cancelled it. */
"cancelled" | 
/**  The arguments make no sense, e.g. deleting the scan root. */
"invalid" | 
/**  A file on this computer could not be written. */
"io";

export type FocusState = {
	node: NodeRef,
	/**  The folder's subtree is fully measured; the GUI clears its spinner. */
	settled: boolean,
};

/**  How far a node's size is known. See [sizes](crate#sizes). */
export type Measure = 
/**
 *  Not listed yet (a folder) or not stat'ed yet (a file). The GUI shows
 *  "measuring…", not 0.
 */
"unknown" | 
/**  A lower bound that climbs as the scan goes on. */
"partial" | 
/**  Everything below is measured. */
"final";

/**
 *  A file or folder of one session's tree. See [nodes, not
 *  paths](crate#nodes-not-paths).
 */
export type NodeRef = {
	session: number,
	id: number,
};

/**  One file or folder in a list. */
export type Row = {
	node: NodeRef,
	/**  Lossy UTF-8, for display only. Actions travel by `node`. */
	name: string,
	kind: EntryKind,
	size: Size,
	measure: Measure,
	/**  Files in the subtree; 1 for a file. */
	files: number,
	/**  Some folder in the subtree could not be read, so the size is low. */
	unreadable: boolean,
	/**
	 *  The containing folder's path, on results that span the tree (search
	 *  hits, largest files), where the name alone does not say where the thing
	 *  lives. `null` inside a single folder's listing.
	 */
	parent: string | null,
	/**  That folder, for "go to". */
	parentNode: NodeRef | null,
};

export type ScanFinished = {
	outcome: ScanOutcome,
	elapsedMs: number,
	/**  Why, when the scan failed. */
	message: string | null,
};

export type ScanOutcome = "complete" | "cancelled" | "failed";

export type ScanPhase = 
/**  Listing the top levels, breadth first. */
"skeleton" | 
/**  Measuring everything, nearest the focused folder first. */
"measuring" | "finished";

/**
 *  Emitted as `scan-progress` about ten times a second while a scan runs,
 *  and once more when it ends.
 */
export type ScanProgress = {
	session: number,
	phase: ScanPhase,
	totals: Totals,
	/**  Folders listed so far, of `known` discovered. */
	listed: number,
	known: number,
	elapsedMs: number,
	/**
	 *  Set once the skeleton is ready: the top levels of the tree are listed
	 *  and every folder in them can be opened.
	 */
	skeleton: SkeletonReady | null,
	/**  The folder last passed to `focus`, and whether it is fully measured. */
	focus: FocusState | null,
	/**  Set on the last event of a scan. */
	finished: ScanFinished | null,
	/**
	 *  The focused folder, refreshed. Only that one, so the payload does not
	 *  grow with the tree. Its sort and row cap are the `by` and `limit` of
	 *  the last `focus` (defaults: disk, 500 rows); `getView` never changes
	 *  them, so a GUI that wants more rows or another sort calls `focus` again.
	 */
	view: View | null,
};

/**  A remembered scan. */
export type ScanRecord = {
	size: Size,
	files: number,
	dirs: number,
	/**  Unix seconds. The GUI formats it. */
	at: number,
};

/**  The answer to `startScan`: the scan is under way. */
export type ScanStarted = {
	/**  The new tree. Every node from an earlier session is now stale. */
	session: number,
	root: NodeRef,
	/**  The root as the user knows it, for the first breadcrumb. */
	path: string,
};

/**  A size in bytes, both ways of counting it. */
export type Size = {
	/**  The sum of file lengths, what a file manager shows. */
	apparent: number,
	/**  Space allocated on the phone's storage, what deleting frees. */
	disk: number,
};

/**  Which [`Size`] a query sorts and sizes by. */
export type SizeMetric = "disk" | "apparent";

export type SkeletonReady = {
	/**  Folders listed when the skeleton phase ended. */
	listed: number,
	/**  Folders known but not yet listed. */
	frontier: number,
	elapsedMs: number,
};

/**  Sums over a tree or subtree. */
export type Totals = {
	size: Size,
	files: number,
	dirs: number,
	/**  Folders that could not be read and files that could not be measured. */
	errors: number,
};

/**  A node plus a bounded slice of its descendants, for the nested treemap. */
export type TreemapNode = {
	node: NodeRef,
	name: string,
	kind: EntryKind,
	size: Size,
	measure: Measure,
	/**  Largest first; empty below the requested depth. */
	children: TreemapNode[],
};

/**  Total size of one broad file category (images, video, …). */
export type TypeGroup = {
	label: string,
	size: Size,
	files: number,
};

/**  One folder's contents, largest first by the requested [`SizeMetric`]. */
export type View = {
	node: NodeRef,
	/**  Display path. */
	path: string,
	size: Size,
	measure: Measure,
	files: number,
	dirs: number,
	unreadable: boolean,
	rows: Row[],
	/**  Rows beyond the requested limit, so the GUI can say "and 412 more". */
	hidden: number,
};
