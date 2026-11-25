import type { Row, SizeMetric } from "../../api";
import { categoryColor } from "../../lib/categories";
import { fileKind, formatBytes, formatDate, plural } from "../../lib/format";
import { sizeOf } from "../../lib/size";
import { Checkbox } from "../common/Checkbox";
import { SizeCell } from "../common/SizeCell";
import { IconCopy, IconTrash } from "../icons";
import { ROW_GRID } from "./columns";
import { RowName } from "./RowName";

export interface RowActions {
  metric: SizeMetric;
  onOpen: (row: Row) => void;
  /** Go to a search hit's folder; only search hits have one to go to. */
  onReveal: (row: Row) => void;
  onToggle: (row: Row) => void;
  onCopy: (row: Row) => void;
  onDelete: (row: Row) => void;
  /** Where Copy puts things, as a short name ("Desktop"); null when the host knows no default. */
  copyTarget: string | null;
  /** A copy is already running; the host runs one at a time. */
  copyBusy: boolean;
}

export function ListRow({
  row,
  index,
  active,
  onActivate,
  share,
  picked,
  highlighted,
  onHoverChange,
  actions,
}: {
  row: Row;
  /** Position in the list; the header is row 1 for assistive tech. */
  index: number;
  /** The list's one tab stop. */
  active: boolean;
  onActivate: () => void;
  /** Its part of the open folder, 0 to 1; null where a share means nothing (search hits). */
  share: number | null;
  picked: boolean;
  /** The pointer is on this row's tile in the map. */
  highlighted: boolean;
  onHoverChange: (hovering: boolean) => void;
  actions: RowActions;
}) {
  const { metric } = actions;
  return (
    <div
      role="row"
      aria-rowindex={index + 2}
      aria-selected={picked}
      data-row-index={index}
      tabIndex={active ? 0 : -1}
      onFocus={onActivate}
      data-hovered={highlighted || undefined}
      data-staged={row.staged || undefined}
      className={`list-row grid items-center gap-x-3 px-3 h-full rounded-lg ${
        highlighted ? "bg-sunk" : picked ? "bg-sunk/60" : "hover:bg-sunk"
      } ${row.staged ? "opacity-60" : ""}`}
      style={{ gridTemplateColumns: ROW_GRID }}
      onPointerEnter={() => onHoverChange(true)}
      onPointerLeave={() => onHoverChange(false)}
    >
      <span role="gridcell" className="flex items-center">
        <Checkbox
          checked={picked}
          label={`Select ${row.name}`}
          onChange={() => actions.onToggle(row)}
        />
      </span>
      <RowName row={row} onOpen={actions.onOpen} onReveal={actions.onReveal} />
      <span role="gridcell" className="text-right text-[13px]">
        <SizeCell
          size={row.size}
          measure={row.measure}
          metric={metric}
          projected={row.staged ? undefined : row.projected}
        />
      </span>
      <span role="gridcell">
        {share !== null && row.measure !== "unknown" && <Share row={row} share={share} />}
      </span>
      <Details row={row} />
      <RowButtons row={row} actions={actions} />
    </div>
  );
}

/** Contents and last change. A folder's date is the newest change anywhere inside it. */
function Details({ row }: { row: Row }) {
  const isDir = row.kind === "dir";
  return (
    <>
      <span role="gridcell" className="text-[12px] text-muted truncate num">
        {isDir ? plural(row.files, "file") : fileKind(row.name)}
      </span>
      <span
        role="gridcell"
        className="text-[12px] text-muted num truncate"
        title={isDir ? "The newest change anywhere inside this folder" : undefined}
      >
        {formatDate(row.changed)}
      </span>
    </>
  );
}

function Share({ row, share }: { row: Row; share: number }) {
  const pct = Math.min(1, Math.max(0, share)) * 100;
  return (
    <span className="flex items-center gap-2" aria-label={`${pct.toFixed(0)}% of this folder`}>
      <span className="flex-1 h-1.5 rounded-full bg-sunk overflow-hidden" aria-hidden>
        <span
          className="block h-full rounded-full"
          style={{ width: `${Math.max(1, pct)}%`, background: categoryColor(row.category) }}
        />
      </span>
      <span className="num text-[11px] text-muted w-9 text-right" aria-hidden>
        {pct < 1 && pct > 0 ? "<1" : pct.toFixed(0)}%
      </span>
    </span>
  );
}

/** Shown on hover or focus only, but always in the tab order, so they exist for keyboard users. */
function RowButtons({ row, actions }: { row: Row; actions: RowActions }) {
  const size =
    row.measure === "unknown"
      ? "size not measured yet"
      : `${row.measure === "partial" ? "at least " : ""}${formatBytes(sizeOf(row.size, actions.metric))}`;
  const target = actions.copyTarget ?? "computer";
  return (
    <span role="gridcell" className="row-actions flex items-center justify-end gap-1">
      <button
        type="button"
        onClick={() => actions.onCopy(row)}
        disabled={actions.copyBusy || actions.copyTarget === null}
        title={actions.copyBusy ? "A copy is already running" : `Copy to ${target}`}
        aria-label={`Copy ${row.name} to ${target}`}
        className="w-9 h-9 rounded-lg inline-flex items-center justify-center text-muted hover:text-fg hover:bg-panel disabled:opacity-40"
      >
        <IconCopy />
      </button>
      <button
        type="button"
        onClick={() => actions.onDelete(row)}
        disabled={row.staged}
        title={
          row.staged ? "Already pending delete" : "Delete (reviewed before anything is removed)"
        }
        aria-label={`Delete ${row.name}, ${size}${row.kind === "dir" ? `, ${plural(row.files, "file")}` : ""}`}
        className="w-9 h-9 rounded-lg inline-flex items-center justify-center text-danger hover:bg-panel disabled:opacity-40"
      >
        <IconTrash />
      </button>
    </span>
  );
}
