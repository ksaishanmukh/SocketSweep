import type { ScanInfo } from "../hooks/useScanSession";
import { formatNumber } from "../lib/format";
import { IconRescan } from "./icons";

/**
 * The scan, in one line beside the totals rather than a screen of its own.
 *
 * While it runs the user can already open folders, so this only says how far
 * along the phone is. After it ends it says how long it took, or, if it did
 * not finish, why the sizes shown are incomplete.
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
  if (scan.running) return <RunningStatus scan={scan} onStop={onStop} />;
  if (scan.outcome === "complete" || scan.outcome === null) {
    return (
      <span className="text-[13px] text-muted" role="status">
        Scan complete in <span className="num">{(scan.elapsedMs / 1000).toFixed(1)} s</span>
      </span>
    );
  }
  return <IncompleteStatus scan={scan} onRescan={onRescan} />;
}

function RunningStatus({ scan, onStop }: { scan: ScanInfo; onStop: () => void }) {
  // Folders listed out of folders discovered so far. The denominator grows as
  // the scan finds more, so this is progress, not a promise.
  const fraction = scan.known > 0 ? Math.min(1, scan.listed / scan.known) : 0;
  const skeleton = scan.phase === "skeleton";
  return (
    <div className="flex items-center gap-3 min-w-0" role="status">
      <div className="flex flex-col gap-1 min-w-0">
        <span className="text-[13px] text-muted whitespace-nowrap">
          {skeleton ? "Listing top levels" : "Measuring"} ·{" "}
          <span className="num" aria-live="off">
            {formatNumber(scan.listed)} of {formatNumber(scan.known)} folders
          </span>
        </span>
        <div className="h-1 w-56 max-w-full rounded-full bg-sunk overflow-hidden" aria-hidden>
          {skeleton || scan.known === 0 ? (
            <div className="h-full rounded-full bg-accent indeterminate-bar" />
          ) : (
            <div
              className="h-full rounded-full bg-accent transition-[width] duration-300"
              style={{ width: `${Math.max(2, fraction * 100)}%` }}
            />
          )}
        </div>
      </div>
      <button
        type="button"
        onClick={onStop}
        className="min-h-9 px-3 rounded-lg border border-line text-[13px] font-medium hover:bg-sunk"
      >
        Stop
      </button>
    </div>
  );
}

function IncompleteStatus({ scan, onRescan }: { scan: ScanInfo; onRescan: () => void }) {
  const failed = scan.outcome === "failed";
  return (
    <div className="flex items-center gap-3 min-w-0" role="status">
      <span className={`text-[13px] min-w-0 truncate ${failed ? "text-danger" : "text-warn"}`}>
        {failed ? "The scan failed" : "The scan was stopped"}
        {scan.message ? `: ${scan.message}` : ""}. Sizes shown are incomplete.
      </span>
      <button
        type="button"
        onClick={onRescan}
        className="min-h-9 px-3 rounded-lg border border-line text-[13px] font-medium hover:bg-sunk inline-flex items-center gap-1.5 shrink-0"
      >
        <IconRescan />
        Rescan
      </button>
    </div>
  );
}
