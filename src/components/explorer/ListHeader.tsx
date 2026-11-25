import type { SortKey, ViewSort } from "../../api";
import { Checkbox } from "../common/Checkbox";
import { COLUMNS, ROW_GRID } from "./columns";

/**
 * The select-all box, acting on the rows shown, and the column headers,
 * which sort. The host sorts the whole folder, not just the loaded rows.
 */
export function ListHeader({
  sort,
  shown,
  picked,
  onSort,
  onSelectAll,
}: {
  sort: ViewSort;
  /** Rows currently in the list. */
  shown: number;
  /** How many of those are picked. */
  picked: number;
  onSort: (key: SortKey) => void;
  onSelectAll: (on: boolean) => void;
}) {
  const all = shown > 0 && picked === shown;
  const some = picked > 0 && !all;
  return (
    <div
      role="row"
      aria-rowindex={1}
      className="grid items-center gap-x-3 px-3 h-10 border-b border-line text-[12px] text-muted"
      style={{ gridTemplateColumns: ROW_GRID }}
    >
      <span role="columnheader" className="flex items-center">
        <Checkbox
          checked={all}
          mixed={some}
          label={all || some ? "Unselect all shown" : `Select all ${shown} shown`}
          onChange={() => onSelectAll(!(all || some))}
        />
      </span>
      {COLUMNS.map((c) => (
        <SortHeader key={c.label} column={c} sort={sort} onSort={onSort} />
      ))}
      <span role="columnheader" className="sr-only">
        Actions
      </span>
    </div>
  );
}

function SortHeader({
  column,
  sort,
  onSort,
}: {
  column: (typeof COLUMNS)[number];
  sort: ViewSort;
  onSort: (key: SortKey) => void;
}) {
  // Size and Share sort the same way; the arrow sits on Size alone.
  const active = sort.key === column.key && column.label !== "Share of this folder";
  const direction = sort.descending ? "descending" : "ascending";
  return (
    <span
      role="columnheader"
      aria-sort={active ? direction : undefined}
      className={column.align === "right" ? "text-right" : ""}
    >
      <button
        type="button"
        onClick={() => onSort(column.key)}
        className={`min-h-8 rounded px-1 -mx-1 hover:text-fg truncate max-w-full ${
          active ? "text-fg font-semibold" : ""
        }`}
      >
        {column.label}
        {active && <span aria-hidden>{sort.descending ? " ↓" : " ↑"}</span>}
      </button>
    </span>
  );
}
