/**
 * Mirrors the serde types in src-tauri. The Rust side is the source of truth;
 * these exist so TypeScript can see the shape.
 *
 * Nodes are addressed by `id`, never by path. The host holds the tree and the
 * byte-exact paths; a path that round-tripped through a JavaScript string could
 * come back subtly different, and it would come back as a delete target.
 */

export interface Device {
  serial: string;
  /** Human-readable model for the picker; falls back to the serial. */
  model: string;
  /** "device", "unauthorized", "offline", … */
  state: string;
  usable: boolean;
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

/** Total bytes per broad file category. */
export interface TypeGroup {
  label: string;
  size: number;
  files: number;
}

export interface Row {
  id: number;
  /** Lossy UTF-8, display only. */
  name: string;
  size: number;
  isDir: boolean;
  files: number;
  /** False while this subtree is still being walked. */
  complete: boolean;
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
  path: string;
  size: number;
  rows: Row[];
  /** Rows beyond the requested limit. */
  hidden: number;
  complete: boolean;
}

export interface Crumb {
  id: number;
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
  view: View | null;
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
  name: string;
  size: number;
  isDir: boolean;
  children: TreemapNode[];
}
