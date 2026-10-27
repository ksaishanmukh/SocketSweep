/**
 * Mirrors the serde types in src-tauri, which are the source of truth.
 *
 * Nodes are addressed by `id`, never by path. The host holds the tree and the
 * byte-exact paths: a path round-tripped through a JavaScript string can come
 * back subtly different, and it comes back as a delete target.
 */

export interface Device {
  serial: string;
  /** Human-readable model for the picker; falls back to the serial. */
  model: string;
  /** "device", "unauthorized", "offline", … */
  state: string;
  usable: boolean;
  /** What to do about a device that cannot be used yet, when the host knows. */
  hint: string | null;
}

export interface Connected {
  serial: string;
  model: string;
  root: string;
  /** Last scan of this device, if it has been seen before. */
  previous: ScanRecord | null;
}

/** A remembered scan, for showing what changed since last time. */
export interface ScanRecord {
  size: number;
  files: number;
  dirs: number;
  /** Unix seconds. */
  at: number;
}

/** Storage owned by one app, summed across its data, obb and media folders. */
export interface AppUsage {
  /** Android package name. Not a display label. */
  package: string;
  size: number;
  files: number;
  /** Largest folder the package owns, for navigating to it. */
  id: number;
  /** Generation of the tree that `id` belongs to. */
  gen: number;
}

/** Total bytes per broad file category. */
export interface TypeGroup {
  label: string;
  size: number;
  files: number;
}

export interface Row {
  id: number;
  /**
   * Generation of the tree this id belongs to. A rescan replaces the tree, so
   * an id from an earlier generation no longer names anything.
   */
  gen: number;
  /** Lossy UTF-8, display only. */
  name: string;
  size: number;
  isDir: boolean;
  files: number;
  /** False while this subtree is still being walked. */
  complete: boolean;
  /**
   * False for a folder the scanner never read. Its 0 bytes means unknown, not
   * empty.
   */
  listed: boolean;
  /**
   * Containing folder, present only on results that span the tree (search hits,
   * largest files) where the name alone does not say where the thing lives.
   */
  parent?: string;
  /** Id of that folder, for navigating to it. */
  parentId?: number;
}

export interface View {
  id: number;
  gen: number;
  path: string;
  size: number;
  rows: Row[];
  /** Rows beyond the requested limit. */
  hidden: number;
  complete: boolean;
}

export interface Crumb {
  id: number;
  gen: number;
  name: string;
}

export interface Stats {
  size: number;
  files: number;
  dirs: number;
  scanning: boolean;
}

export interface Deleted {
  items: number;
  stats: Stats;
  /** The nearest surviving folder: the open one, or its closest ancestor if it was deleted. */
  view: View;
}

/** Payload of the `scan-progress` event, emitted ~10x/second during a scan. */
export interface ScanProgress {
  stats: Stats;
  /** Only the view the frontend said it was watching, so this stays small. */
  view: View | null;
}

export const ROOT_ID = 0;

/** A node plus a bounded slice of its descendants, for the nested treemap. */
export interface TreemapNode {
  id: number;
  gen: number;
  name: string;
  size: number;
  isDir: boolean;
  /** False for a folder the scanner never read. */
  listed: boolean;
  children: TreemapNode[];
}
