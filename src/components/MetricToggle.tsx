import type { SizeMetric } from "../api";
import { METRIC_HINT, METRIC_LABEL } from "../lib/size";

const METRICS: SizeMetric[] = ["disk", "apparent"];

/** Disk usage (what deleting frees) or apparent size (what a file manager shows). */
export function MetricToggle({
  metric,
  onChange,
}: {
  metric: SizeMetric;
  onChange: (m: SizeMetric) => void;
}) {
  return (
    <div
      role="radiogroup"
      aria-label="Size shown"
      className="flex items-center rounded-md border border-zinc-800 bg-zinc-900/60 p-0.5"
    >
      {METRICS.map((m) => (
        <button
          key={m}
          role="radio"
          aria-checked={metric === m}
          title={METRIC_HINT[m]}
          onClick={() => onChange(m)}
          className={`px-2 py-0.5 rounded text-[11px] font-medium transition-colors
            focus-visible:outline-2 focus-visible:outline-accent-500 ${
              metric === m ? "bg-zinc-700 text-zinc-100" : "text-zinc-500 hover:text-zinc-300"
            }`}
        >
          {METRIC_LABEL[m]}
        </button>
      ))}
    </div>
  );
}
