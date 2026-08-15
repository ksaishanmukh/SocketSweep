/**
 * Typed wrappers over the Tauri command surface.
 *
 * Every command returns a real value rather than a JSON string. The previous
 * bridge returned `String` from every command and the frontend called
 * `JSON.parse` on the result in three places.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Connected,
  Crumb,
  Deleted,
  Device,
  Row,
  ScanProgress,
  Stats,
  TreemapNode,
  View,
} from "./types";

export const listDevices = () => invoke<Device[]>("list_devices");

export const connect = (serial?: string, root?: string) =>
  invoke<Connected>("connect", { serial, root });

export const disconnect = () => invoke<void>("disconnect");

/** Resolves when the walk finishes. Progress arrives via `onScanProgress`. */
export const scan = (root?: string) => invoke<Stats>("scan", { root });

/**
 * Rows for one directory, largest first.
 *
 * Also tells the host which directory is on screen, so scan-progress pushes
 * stay scoped to it.
 */
export const getView = (id?: number, limit?: number) => invoke<View>("get_view", { id, limit });

export const getBreadcrumbs = (id: number) => invoke<Crumb[]>("get_breadcrumbs", { id });

/** A depth-limited slice of the tree, so the treemap can nest without the
 * frontend holding the tree. */
export const getTreemap = (id?: number, depth?: number) =>
  invoke<TreemapNode>("get_treemap", { id, depth });

export const getStats = () => invoke<Stats>("get_stats");

export const largestFiles = (limit?: number) => invoke<Row[]>("largest_files", { limit });

export const search = (query: string, limit?: number) => invoke<Row[]>("search", { query, limit });

export const deleteNode = (id: number) => invoke<Deleted>("delete", { id });

// ── Events ──────────────────────────────────────────────────────────────────

export const onScanProgress = (fn: (p: ScanProgress) => void): Promise<UnlistenFn> =>
  listen<ScanProgress>("scan-progress", (e) => fn(e.payload));

export const onScanComplete = (fn: (s: Stats) => void): Promise<UnlistenFn> =>
  listen<Stats>("scan-complete", (e) => fn(e.payload));

/** Tauri surfaces command failures as thrown strings. */
export function errorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}
