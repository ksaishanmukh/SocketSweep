import type { Row, SizeMetric } from "../../api";
import type { Selection } from "../../hooks/useSelection";
import { categoryColor } from "../../lib/categories";
import { formatDate } from "../../lib/format";
import { targetOfRow } from "../../lib/selection";
import { nodeKey } from "../../lib/size";
import { Card } from "../common/Card";
import { Checkbox } from "../common/Checkbox";
import { SizeCell } from "../common/SizeCell";
import { PendingMarker } from "../explorer/RowName";

/** The largest files anywhere on the phone, each with the folder it lives in. */
export function BiggestFiles({
  rows,
  metric,
  selection,
  running,
  onReveal,
}: {
  rows: Row[];
  metric: SizeMetric;
  selection: Selection;
  running: boolean;
  onReveal: (row: Row) => void;
}) {
  return (
    <Card id="biggest-files" title="Biggest files" className="col-span-12 lg:col-span-7">
      {rows.length === 0 ? (
        <p className="text-[13px] text-muted">
          {running ? "Looking for files…" : "No files found."}
        </p>
      ) : (
        <ul className="-mx-2">
          {rows.map((row) => (
            <BiggestFile
              key={nodeKey(row.node)}
              row={row}
              metric={metric}
              selection={selection}
              onReveal={onReveal}
            />
          ))}
        </ul>
      )}
    </Card>
  );
}

function BiggestFile({
  row,
  metric,
  selection,
  onReveal,
}: {
  row: Row;
  metric: SizeMetric;
  selection: Selection;
  onReveal: (row: Row) => void;
}) {
  const target = targetOfRow(row, row.parent ?? "");
  return (
    <li
      className={`flex items-center gap-3 px-2 py-1.5 rounded-lg hover:bg-sunk min-h-11 ${
        row.staged ? "opacity-60" : ""
      }`}
    >
      <Checkbox
        checked={selection.has(target.key)}
        label={`Select ${row.name}`}
        onChange={() => selection.toggle(target)}
      />
      <span
        className="w-2.5 h-2.5 rounded-full shrink-0"
        style={{ background: categoryColor(row.category) }}
        aria-hidden
      />
      <div className="min-w-0 flex-1">
        <div
          className={`truncate text-[14px] ${row.staged ? "line-through" : ""}`}
          title={row.name}
        >
          {row.name}
        </div>
        {row.parent && (
          <button
            type="button"
            onClick={() => onReveal(row)}
            title={`Show ${row.parent} in Explorer`}
            className="block max-w-full truncate text-left text-[12px] text-muted hover:text-accent hover:underline"
          >
            <span className="font-mono">{row.parent}</span> · {formatDate(row.changed)}
          </button>
        )}
      </div>
      {row.staged && <PendingMarker />}
      <SizeCell
        size={row.size}
        measure={row.measure}
        metric={metric}
        projected={row.staged ? undefined : row.projected}
        className="text-[13px]"
      />
    </li>
  );
}
