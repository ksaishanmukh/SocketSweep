import type { Overview, SizeMetric } from "../../api";
import { formatBytes, plural } from "../../lib/format";
import { sizeOf } from "../../lib/size";
import { Card } from "../common/Card";

/**
 * What the pending-delete queue would free, and what the phone would hold
 * afterwards. Shown only while something is queued.
 */
export function WouldFreeCard({ overview, metric }: { overview: Overview; metric: SizeMetric }) {
  const { staged, totals, projected } = overview;
  if (staged.items === 0) return null;
  const lowerBound = staged.measure !== "final";
  return (
    <Card
      id="would-free"
      title="Pending delete"
      className="col-span-12"
      aside={<span className="text-[13px] text-muted">{plural(staged.items, "item")}</span>}
    >
      <p className="text-[15px]" role="status">
        Would free{" "}
        <span className="num font-semibold">
          {lowerBound ? "at least " : ""}
          {formatBytes(sizeOf(staged.size, metric))}
        </span>
        <span className="text-muted num">
          {" "}
          · the phone would go from {formatBytes(sizeOf(totals.size, metric))} to{" "}
          {formatBytes(sizeOf(projected, metric))}
        </span>
      </p>
    </Card>
  );
}
