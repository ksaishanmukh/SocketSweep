import type { ScanSession } from "../../../hooks/useScanSession";
import { formatBytes, formatNumber } from "../../../lib/format";
import { sizeOf } from "../../../lib/size";
import { MetricToggle } from "../../MetricToggle";
import { ModeTabs } from "../../ModeTabs";
import { SinceLastScan } from "../../SinceLastScan";

/** Totals in one line, so the vertical space goes to the treemap. */
export function TotalsBar({ session, running }: { session: ScanSession; running: boolean }) {
  const { totals, scanInfo, previous, metric, mode } = session;
  return (
    <div className="flex items-center gap-3 px-5 py-2 text-xs shrink-0 border-b border-zinc-800/60">
      {totals && (
        <>
          <span className="font-mono font-bold text-accent-400 tabular-nums text-sm">
            {running && <span className="counting-dot" aria-hidden />}
            {formatBytes(sizeOf(totals.size, metric))}
            {running && <span className="sr-only"> (still counting)</span>}
          </span>
          <span className="text-zinc-700">·</span>
          <span className="text-zinc-400 tabular-nums">{formatNumber(totals.files)} files</span>
          <span className="text-zinc-700">·</span>
          <span className="text-zinc-400 tabular-nums">{formatNumber(totals.dirs)} folders</span>
          {totals.errors > 0 && (
            <>
              <span className="text-zinc-700">·</span>
              <span
                className="text-amber-500/80 tabular-nums"
                title="Folders that could not be read and files that could not be measured"
              >
                {formatNumber(totals.errors)} unreadable
              </span>
            </>
          )}
          {scanInfo && !running && (
            <>
              <span className="text-zinc-700">·</span>
              <span className="text-zinc-500 tabular-nums">
                {(scanInfo.elapsedMs / 1000).toFixed(1)}s
              </span>
            </>
          )}
          <SinceLastScan
            previous={previous}
            totals={totals}
            metric={metric}
            incomplete={scanInfo?.outcome !== "complete"}
          />
        </>
      )}

      <div className="flex-1" />
      <MetricToggle metric={metric} onChange={session.setMetric} />
      <ModeTabs mode={mode} onChange={session.setMode} />
    </div>
  );
}
