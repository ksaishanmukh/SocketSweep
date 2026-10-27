import type { Measure, Size, SizeMetric } from "../api";
import { formatBytes } from "../lib/format";
import { sizeOf } from "../lib/size";

/**
 * A size, honest about how much of it is known.
 *
 * - `unknown`: the host has not measured it yet. Shown as "measuring…", never
 *   as 0, which would read as "empty".
 * - `partial`: a lower bound that is still climbing. The number is shown with a
 *   small pulsing dot.
 * - `final`: the plain number.
 */
export function SizeCell({
  size,
  measure,
  metric,
  className = "",
}: {
  size: Size;
  measure: Measure;
  metric: SizeMetric;
  className?: string;
}) {
  if (measure === "unknown") {
    return (
      <span className={`measuring ${className}`} title="Not measured yet" aria-label="Measuring">
        measuring…
      </span>
    );
  }
  const text = formatBytes(sizeOf(size, metric));
  if (measure === "partial") {
    return (
      <span
        className={className}
        title="Still counting: this is a lower bound that is still growing"
        data-measure="partial"
      >
        <span className="counting-dot" aria-hidden />
        {text}
        <span className="sr-only"> (still counting)</span>
      </span>
    );
  }
  return <span className={className}>{text}</span>;
}
