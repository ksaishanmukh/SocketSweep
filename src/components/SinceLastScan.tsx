import type { ScanRecord, SizeMetric, Totals } from "../api";
import { formatBytes } from "../lib/format";
import { sizeOf } from "../lib/size";

/**
 * Change since the previous scan of this device.
 *
 * Storage is somewhere people come back to, so the useful question is usually
 * "what grew" rather than "what is the total".
 */
export function SinceLastScan({
  previous,
  totals,
  metric,
  incomplete,
}: {
  previous: ScanRecord | null;
  totals: Totals;
  metric: SizeMetric;
  /** A scan still counting, stopped or failed would compare a partial total against a final one. */
  incomplete: boolean;
}) {
  if (!previous || incomplete) return null;

  const delta = sizeOf(totals.size, metric) - sizeOf(previous.size, metric);
  // Below a megabyte is noise from caches and logs, not something to report.
  if (Math.abs(delta) < 1024 * 1024) return null;

  const grew = delta > 0;
  return (
    <span
      className={`tabular-nums ${grew ? "text-amber-400/90" : "text-emerald-400/90"}`}
      title={`Previous scan: ${formatBytes(sizeOf(previous.size, metric))} on ${new Date(
        previous.at * 1000,
      ).toLocaleString()}`}
    >
      {grew ? "+" : "−"}
      {formatBytes(Math.abs(delta))} since {relative(previous.at)}
    </span>
  );
}

function relative(unixSeconds: number): string {
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - unixSeconds);
  const minutes = Math.floor(seconds / 60);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;

  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;

  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  return new Date(unixSeconds * 1000).toLocaleDateString();
}
