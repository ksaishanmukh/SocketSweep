import type { Measure, Size, SizeMetric } from "../../api";
import { formatBytes } from "../../lib/format";
import { sizeOf } from "../../lib/size";
import { afterDelete } from "../../lib/staged";

/**
 * A size, honest about how much of it is known.
 *
 * - `unknown`: the host has not measured it yet. Shown as "measuring…", never
 *   as 0, which would read as "empty".
 * - `partial`: a lower bound that is still climbing, with a pulsing amber dot.
 * - `final`: the plain number.
 *
 * With `projected` (what the size becomes once the pending deletes are carried
 * out) it reads "now → after" wherever the two differ.
 */
export function SizeCell({
  size,
  measure,
  metric,
  projected,
  className = "",
}: {
  size: Size;
  measure: Measure;
  metric: SizeMetric;
  projected?: Size;
  className?: string;
}) {
  if (measure === "unknown") {
    return (
      <span className={`num measuring ${className}`} title="Not measured yet">
        measuring…
      </span>
    );
  }
  const change = projected ? afterDelete(size, projected, measure, metric) : null;
  const text = change ? (
    <>
      {formatBytes(change.now)}
      <span aria-hidden> → </span>
      <span className="sr-only"> becomes </span>
      <span className="font-medium">{formatBytes(change.after)}</span>
      <span className="sr-only"> after the pending delete</span>
    </>
  ) : (
    formatBytes(sizeOf(size, metric))
  );
  if (measure === "partial") {
    return (
      <span
        className={`num ${className}`}
        title="Still counting: this is a lower bound that is still growing"
        data-measure="partial"
      >
        <span className="counting-dot" aria-hidden />
        {text}
        <span className="sr-only"> (still counting)</span>
      </span>
    );
  }
  return <span className={`num ${className}`}>{text}</span>;
}
