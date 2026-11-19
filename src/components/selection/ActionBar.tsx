import type { SizeMetric } from "../../api";
import { formatBytes, formatNumber, plural } from "../../lib/format";
import { folderName } from "../../lib/path";
import { totalOf, type Target } from "../../lib/selection";
import { IconCopy, IconTrash } from "../icons";

/** What is picked, and what can be done with all of it at once. */
export function ActionBar({
  items,
  metric,
  copyDestination,
  copyBusy,
  onClear,
  onCopy,
  onDelete,
}: {
  items: Target[];
  metric: SizeMetric;
  copyDestination: string | null;
  copyBusy: boolean;
  onClear: () => void;
  onCopy: () => void;
  onDelete: () => void;
}) {
  const total = totalOf(items, metric);
  return (
    <div
      role="region"
      aria-label="Selection"
      className="shrink-0 border-t border-line bg-panel rise-in"
    >
      <div className="max-w-[1320px] mx-auto px-6 py-2.5 flex items-center gap-3">
        <span className="text-[14px] font-medium" aria-live="polite">
          {formatNumber(items.length)} selected ·{" "}
          <span className="num">
            {total.uncertain ? "at least " : ""}
            {formatBytes(total.bytes)}
          </span>
        </span>
        <button
          type="button"
          onClick={onClear}
          className="min-h-9 px-3 rounded-lg text-[13px] text-muted hover:text-fg hover:bg-sunk"
        >
          Clear
        </button>
        <div className="flex-1" />
        <CopyButton destination={copyDestination} busy={copyBusy} onCopy={onCopy} />
        <button
          type="button"
          onClick={onDelete}
          title="Adds to the pending delete; nothing is removed until you confirm"
          className="min-h-9 px-3 rounded-lg text-[13px] font-semibold bg-danger text-on-danger inline-flex items-center gap-2 hover:opacity-90"
        >
          <IconTrash />
          Delete {plural(items.length, "item")}
        </button>
      </div>
    </div>
  );
}

/** Named for the real destination folder, so it is clear up front where things will land. */
function CopyButton({
  destination,
  busy,
  onCopy,
}: {
  destination: string | null;
  busy: boolean;
  onCopy: () => void;
}) {
  const target = destination ? folderName(destination) : null;
  const why = busy
    ? "A copy is already running"
    : destination === null
      ? "This computer has no default folder to copy to"
      : `Copy to ${destination}`;
  return (
    <button
      type="button"
      onClick={onCopy}
      disabled={busy || destination === null}
      title={why}
      className="min-h-9 px-3 rounded-lg border border-line text-[13px] font-medium hover:bg-sunk inline-flex items-center gap-2 disabled:opacity-50"
    >
      <IconCopy />
      Copy to {target ?? "computer"}
    </button>
  );
}
