import type { CopyState } from "../hooks/useScanSession";
import { formatBytes, formatNumber } from "../lib/format";

/**
 * Progress of "Copy to computer", from `copy-progress` events.
 *
 * Totals are still growing while the phone lists what to copy
 * (`totalsKnown` false), so the bar is indeterminate until they settle.
 */
export function CopyBar({ copy, onCancel }: { copy: CopyState; onCancel: () => void }) {
  const p = copy.progress;
  const determinate = p !== null && p.totalsKnown && p.bytesTotal > 0;
  const fraction = determinate ? Math.min(1, p.bytesDone / p.bytesTotal) : 0;

  let detail = "Preparing…";
  if (p) {
    detail = p.totalsKnown
      ? `${formatNumber(p.filesDone)} of ${formatNumber(p.filesTotal)} files · ${formatBytes(
          p.bytesDone,
        )} of ${formatBytes(p.bytesTotal)}`
      : `Counting files… ${formatNumber(p.filesTotal)} so far`;
  }

  return (
    <div className="shrink-0 border-t border-line bg-panel" role="status">
      <div className="flex items-center gap-3 px-5 py-1.5 text-[11px] text-muted">
        <span className="font-medium text-fg truncate max-w-[16rem]">
          {copy.cancelling ? "Stopping copy of" : "Copying"} {copy.name}
        </span>
        <span className="tabular-nums text-muted truncate" aria-live="off">
          {detail}
        </span>
        {p?.current && (
          <span className="font-mono text-faint truncate min-w-0 flex-1" title={p.current}>
            {p.current}
          </span>
        )}
        {!p?.current && <span className="flex-1" />}
        {p && <span className="text-faint truncate max-w-[14rem]">to {p.destination}</span>}
        <button
          onClick={onCancel}
          disabled={copy.cancelling}
          className="px-2 py-0.5 rounded border border-line text-muted hover:text-fg
            hover:bg-sunk disabled:opacity-50"
        >
          {copy.cancelling ? "Stopping…" : "Cancel"}
        </button>
      </div>
      <div
        className="h-0.5 bg-sunk overflow-hidden"
        role="progressbar"
        aria-label="Copy progress"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={determinate ? Math.round(fraction * 100) : undefined}
      >
        {determinate ? (
          <div
            className="h-full bg-accent transition-[width] duration-200"
            style={{ width: `${fraction * 100}%` }}
          />
        ) : (
          <div className="h-full bg-accent indeterminate-bar" />
        )}
      </div>
    </div>
  );
}
