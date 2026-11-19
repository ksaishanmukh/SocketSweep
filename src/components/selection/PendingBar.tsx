import { useEffect, useState } from "react";
import type { NodeRef, SizeMetric, StagedItem, StagedSummary, StagedTotals } from "../../api";
import { formatBytes, plural } from "../../lib/format";
import { nodeKey, sizeOf } from "../../lib/size";
import { IconTrash } from "../icons";

/**
 * The pending-delete queue, shown for as long as it holds anything, wherever
 * the user browses. Nothing is removed from the phone until "Delete now…" is
 * confirmed; Review lists what is queued, each item with an Undo.
 */
export function PendingBar({
  totals,
  metric,
  load,
  onUndo,
  onClear,
  onDeleteNow,
}: {
  totals: StagedTotals;
  metric: SizeMetric;
  /** Fetches the queued items, for the review list. */
  load: () => Promise<StagedSummary | null>;
  onUndo: (node: NodeRef) => void;
  onClear: () => void;
  onDeleteNow: () => void;
}) {
  const [reviewing, setReviewing] = useState(false);
  const items = usePendingItems(reviewing, totals, load);
  const uncertain = totals.measure !== "final";
  return (
    <div
      role="region"
      aria-label="Pending delete"
      className="relative shrink-0 border-t border-line bg-panel rise-in"
      onKeyDown={(e) => {
        if (e.key === "Escape" && reviewing) setReviewing(false);
      }}
    >
      {reviewing && <ReviewList items={items} metric={metric} onUndo={onUndo} />}
      <div className="max-w-[1320px] mx-auto px-6 py-2.5 flex items-center gap-3">
        <span className="text-[14px] font-medium" aria-live="polite">
          Pending delete · {plural(totals.items, "item")} ·{" "}
          <span className="num">
            {uncertain ? "at least " : ""}
            {formatBytes(sizeOf(totals.size, metric))}
          </span>
        </span>
        <button
          type="button"
          onClick={() => setReviewing((open) => !open)}
          aria-expanded={reviewing}
          aria-controls="pending-delete-list"
          className="min-h-9 px-3 rounded-lg border border-line text-[13px] font-medium hover:bg-sunk"
        >
          Review
        </button>
        <button
          type="button"
          onClick={onClear}
          className="min-h-9 px-3 rounded-lg text-[13px] text-muted hover:text-fg hover:bg-sunk"
        >
          Clear
        </button>
        <div className="flex-1" />
        <button
          type="button"
          onClick={onDeleteNow}
          className="min-h-9 px-3 rounded-lg text-[13px] font-semibold bg-danger text-on-danger inline-flex items-center gap-2 hover:opacity-90"
        >
          <IconTrash />
          Delete now…
        </button>
      </div>
    </div>
  );
}

/** The queued items while the review list is open, fetched again whenever the queue changes. */
function usePendingItems(
  open: boolean,
  totals: StagedTotals,
  load: () => Promise<StagedSummary | null>,
): StagedItem[] | null {
  const [items, setItems] = useState<StagedItem[] | null>(null);
  useEffect(() => {
    if (!open) return;
    let current = true;
    void load().then((summary) => {
      if (current && summary) setItems(summary.items);
    });
    return () => {
      current = false;
    };
    // `totals` is the signal that the queue changed.
  }, [open, totals, load]);
  return open ? items : null;
}

function ReviewList({
  items,
  metric,
  onUndo,
}: {
  items: StagedItem[] | null;
  metric: SizeMetric;
  onUndo: (node: NodeRef) => void;
}) {
  return (
    <div className="absolute bottom-full inset-x-0 border-t border-line bg-panel shadow-2xl">
      <div className="max-w-[1320px] mx-auto px-6 py-2 max-h-72 overflow-y-auto">
        {items === null ? (
          <p className="py-2 text-[13px] text-muted">Loading…</p>
        ) : (
          <ul id="pending-delete-list" aria-label="Items pending delete">
            {items.map((item) => (
              <li key={nodeKey(item.node)} className="flex items-center gap-3 py-1 min-h-11">
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[14px]" title={item.name}>
                    {item.name}
                  </span>
                  <span className="block truncate font-mono text-[11px] text-muted">
                    {item.path}
                  </span>
                </span>
                <span className="num text-[13px] shrink-0">
                  {item.measure === "unknown"
                    ? "not measured"
                    : `${item.measure === "partial" ? "at least " : ""}${formatBytes(sizeOf(item.size, metric))}`}
                </span>
                <button
                  type="button"
                  onClick={() => onUndo(item.node)}
                  aria-label={`Remove ${item.name} from pending delete`}
                  className="min-h-9 px-3 rounded-lg border border-line text-[13px] hover:bg-sunk"
                >
                  Undo
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
