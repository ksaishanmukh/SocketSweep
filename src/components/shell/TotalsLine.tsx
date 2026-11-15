import type { SizeMetric } from "../../api";
import type { ScanSession } from "../../hooks/useScanSession";
import { formatBytes, plural } from "../../lib/format";
import { METRIC_HINT, METRIC_LABEL, sizeOf } from "../../lib/size";
import { Segmented } from "../common/Segmented";
import { ScanStatus } from "../ScanStatus";

const METRICS: { value: SizeMetric; label: string }[] = [
  { value: "disk", label: METRIC_LABEL.disk },
  { value: "apparent", label: METRIC_LABEL.apparent },
];

/** The whole phone in one line: total, counts, how the scan is going, and which size is shown. */
export function TotalsLine({
  session,
  onShowUnreadable,
}: {
  session: ScanSession;
  onShowUnreadable: () => void;
}) {
  const { totals, scanInfo, metric, overview } = session;
  const running = scanInfo?.running ?? false;
  const unreadable = overview?.unreadable.length ?? 0;

  return (
    <div className="flex flex-wrap items-center gap-x-5 gap-y-2 px-6 pb-3">
      {totals && (
        <div className="flex items-baseline gap-3">
          <span className="num text-[26px] font-semibold leading-none">
            {running && <span className="counting-dot" aria-hidden />}
            {formatBytes(sizeOf(totals.size, metric))}
            {running && <span className="sr-only"> (still counting)</span>}
          </span>
          <span className="text-[13px] text-muted num">
            {plural(totals.files, "file")} · {plural(totals.dirs, "folder")}
          </span>
        </div>
      )}

      {scanInfo && (
        <ScanStatus
          scan={scanInfo}
          onStop={session.cancelScan}
          onRescan={() => void session.scan()}
        />
      )}

      {unreadable > 0 && (
        <button
          type="button"
          onClick={onShowUnreadable}
          className="text-[13px] text-warn underline underline-offset-2 min-h-9"
        >
          {plural(unreadable, "folder")} couldn’t be read
        </button>
      )}

      <div className="flex-1" />
      <Segmented
        label="Size shown"
        options={METRICS}
        value={metric}
        onChange={session.controls.setMetric}
        titles={METRIC_HINT}
      />
    </div>
  );
}
