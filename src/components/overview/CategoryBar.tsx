import type { Category, CategoryUsage, SizeMetric } from "../../api";
import { CATEGORY_LABEL, categoryColor } from "../../lib/categories";
import { formatBytes } from "../../lib/format";
import { sizeOf } from "../../lib/size";
import { Card } from "../common/Card";

/**
 * "What's using the space": one segment per category, as wide as its bytes.
 * Every segment and legend entry is a button that opens the Explorer at the
 * top, narrowed to that type.
 */
export function CategoryBar({
  categories,
  metric,
  onPick,
}: {
  categories: CategoryUsage[];
  metric: SizeMetric;
  onPick: (category: Category) => void;
}) {
  const shown = categories.filter((c) => sizeOf(c.size, metric) > 0);
  return (
    <Card id="space-by-type" title="What’s using the space" className="col-span-12">
      {shown.length === 0 ? (
        <p className="text-[13px] text-muted">Nothing measured yet.</p>
      ) : (
        <>
          <div className="flex gap-[3px] h-9 py-1" role="group" aria-label="Space by type">
            {shown.map((c) => (
              <button
                key={c.category}
                type="button"
                className="cat-segment h-full min-w-[6px]"
                style={{ flexGrow: sizeOf(c.size, metric), background: categoryColor(c.category) }}
                onClick={() => onPick(c.category)}
                aria-label={`${CATEGORY_LABEL[c.category]}, ${formatBytes(sizeOf(c.size, metric))}. Show in Explorer`}
                title={`${CATEGORY_LABEL[c.category]} · ${formatBytes(sizeOf(c.size, metric))}`}
              />
            ))}
          </div>
          <ul className="flex flex-wrap gap-x-2 gap-y-1 mt-3">
            {shown.map((c) => (
              <li key={c.category}>
                <button
                  type="button"
                  onClick={() => onPick(c.category)}
                  className="inline-flex items-center gap-2 min-h-9 px-2 rounded-lg hover:bg-sunk text-[13px]"
                >
                  <span
                    className="w-2.5 h-2.5 rounded-full"
                    style={{ background: categoryColor(c.category) }}
                    aria-hidden
                  />
                  <span>{CATEGORY_LABEL[c.category]}</span>
                  <span className="num text-muted">{formatBytes(sizeOf(c.size, metric))}</span>
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
    </Card>
  );
}
