import type { SinceLastScan, SizeMetric } from "../../api";
import { formatBytes, formatDate, formatDelta, formatNumber } from "../../lib/format";
import { sizeOf } from "../../lib/size";
import { Card } from "../common/Card";

/**
 * Change since the previous finished scan of this phone. Storage is
 * somewhere people come back to, so "what grew" is often the real question.
 */
export function SinceLastScanCard({
  since,
  model,
  metric,
  running,
}: {
  since: SinceLastScan | null;
  model: string | null;
  metric: SizeMetric;
  /** A total still counting is compared against a final one, so the figure will still move. */
  running: boolean;
}) {
  return (
    <Card id="since-last-scan" title="Since last scan">
      {since ? (
        <Delta since={since} metric={metric} running={running} />
      ) : (
        <p className="text-[13px] text-muted">
          This is the first scan of {model ?? "this phone"} on this computer. Next time, this shows
          what grew and what shrank.
        </p>
      )}
    </Card>
  );
}

function Delta({
  since,
  metric,
  running,
}: {
  since: SinceLastScan;
  metric: SizeMetric;
  running: boolean;
}) {
  const bytes = metric === "disk" ? since.diskDelta : since.apparentDelta;
  const files = since.filesDelta;
  return (
    <div className="text-[13px]">
      <p>
        <span
          className={`num text-[20px] font-semibold ${bytes > 0 ? "text-warn" : bytes < 0 ? "text-ok" : ""}`}
        >
          {formatDelta(bytes)}
        </span>{" "}
        <span className="text-muted">
          and {files >= 0 ? "+" : "−"}
          {formatNumber(Math.abs(files))} files
        </span>
      </p>
      <p className="text-muted mt-1">
        Compared with {formatDate(since.previous.at)}, when it held{" "}
        <span className="num">{formatBytes(sizeOf(since.previous.size, metric))}</span>.
        {running && " The scan is still counting, so this will change."}
      </p>
    </div>
  );
}
