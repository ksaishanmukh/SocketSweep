import type { ScanInfo } from "../hooks/useScanSession";
import { formatNumber } from "../lib/format";

/**
 * The scan, as a slim strip above the browser rather than a screen of its own.
 *
 * While it runs the user can already open folders, so this only says how far
 * along the phone is. After it ends it stays only if it did not finish, to
 * explain why the sizes shown are incomplete.
 */
export function ScanStatus({
  scan,
  onStop,
  onRescan,
}: {
  scan: ScanInfo;
  onStop: () => void;
  onRescan: () => void;
}) {
  if (scan.running) {
    // Folders listed out of folders discovered so far. The denominator grows as
    // the scan finds more, so this is progress, not a promise.
    const fraction = scan.known > 0 ? Math.min(1, scan.listed / scan.known) : 0;
    const skeleton = scan.phase === "skeleton";
    return (
      <div className="shrink-0 border-b border-zinc-800/60" role="status">
        <div className="flex items-center gap-3 px-5 py-1.5 text-[11px] text-zinc-400">
          <span className="font-medium text-zinc-300">
            {skeleton ? "Listing top levels…" : "Measuring sizes…"}
          </span>
          <span className="tabular-nums text-zinc-500" aria-live="off">
            {formatNumber(scan.listed)} of {formatNumber(scan.known)} folders
          </span>
          <span className="tabular-nums text-zinc-600">{(scan.elapsedMs / 1000).toFixed(1)}s</span>
          <span className="flex-1" />
          <button
            onClick={onStop}
            className="px-2 py-0.5 rounded border border-zinc-700/60 text-zinc-400 hover:text-zinc-100
              hover:bg-zinc-800 focus-visible:outline-2 focus-visible:outline-accent-500"
          >
            Stop
          </button>
        </div>
        <div className="h-0.5 bg-zinc-800 overflow-hidden" aria-hidden>
          {skeleton || scan.known === 0 ? (
            <div className="h-full bg-accent-500 indeterminate-bar" />
          ) : (
            <div
              className="h-full bg-accent-500 transition-[width] duration-300"
              style={{ width: `${Math.max(2, fraction * 100)}%` }}
            />
          )}
        </div>
      </div>
    );
  }

  if (scan.outcome === "complete" || scan.outcome === null) return null;

  const failed = scan.outcome === "failed";
  return (
    <div
      className={`shrink-0 flex items-center gap-3 px-5 py-1.5 text-[11px] border-b ${
        failed
          ? "bg-red-950/40 border-red-900/50 text-red-300"
          : "bg-amber-950/30 border-amber-900/40 text-amber-300"
      }`}
      role="status"
    >
      <span className="min-w-0 flex-1 truncate">
        {failed ? "The scan failed" : "The scan was stopped"}
        {scan.message ? `: ${scan.message}` : ""}. Sizes shown are incomplete.
      </span>
      <button
        onClick={onRescan}
        className="px-2 py-0.5 rounded border border-current/30 hover:bg-white/5 shrink-0
          focus-visible:outline-2 focus-visible:outline-accent-500"
      >
        ↻ Rescan
      </button>
    </div>
  );
}
