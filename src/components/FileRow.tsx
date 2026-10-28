import type { Row, SizeMetric } from "../api";
import { formatBytes, formatNumber } from "../lib/format";
import { sizeOf } from "../lib/size";
import { IconDownload, IconFile, IconFolder, IconTrash } from "./icons";
import { SizeBar } from "./SizeBar";
import { SizeCell } from "./SizeCell";

export interface FileRowActions {
  metric: SizeMetric;
  onOpen: (row: Row) => void;
  onDelete: (row: Row) => void;
  /** "Copy to computer". Omitted, the button is not shown. */
  onCopy?: (row: Row) => void;
  /** A copy is already running; the host runs one at a time. */
  copyDisabled?: boolean;
  /** Where Copy puts things, as a short name for the label ("Desktop"). */
  copyTarget?: string;
  /** Navigate to a row's containing folder. Only meaningful for cross-tree results. */
  onReveal?: (row: Row) => void;
  /**
   * The bar costs 96px. In the narrow side panel that is the difference
   * between reading "Documents" and reading "Doc…", and the treemap beside it
   * already shows proportion.
   */
  showBar?: boolean;
}

export function FileRow({
  row,
  totalSize,
  metric,
  onOpen,
  onDelete,
  onCopy,
  copyDisabled = false,
  copyTarget = "computer",
  onReveal,
  showBar = true,
}: FileRowActions & { row: Row; totalSize: number }) {
  const isDir = row.kind === "dir";
  // Not measured yet: the size is unknown, not 0, so there is nothing to draw a bar for.
  const unknown = row.measure === "unknown";
  const ratio = totalSize > 0 && !unknown ? sizeOf(row.size, metric) / totalSize : 0;
  const sizeLabel = unknown
    ? "size not measured yet"
    : `${row.measure === "partial" ? "at least " : ""}${formatBytes(sizeOf(row.size, metric))}`;

  return (
    <div
      className="file-row flex items-center gap-3 px-3 h-full rounded-md group select-none"
      onKeyDown={(e) => {
        // Delete on the focused row, so removing something never needs a mouse.
        if (e.key === "Delete") {
          e.preventDefault();
          onDelete(row);
        }
      }}
    >
      <span className={isDir ? "text-accent-400" : "text-zinc-500"}>
        {isDir ? <IconFolder /> : <IconFile />}
      </span>

      <RowName row={row} onOpen={onOpen} onReveal={onReveal} />

      {row.unreadable && (
        <span
          className="text-[10px] text-amber-500/80"
          title="Some folders in here could not be read, so this size is low"
          aria-label="Some folders could not be read"
        >
          ⚠
        </span>
      )}

      {showBar && (unknown ? <span className="w-24 flex-shrink-0" /> : <SizeBar ratio={ratio} />)}

      <SizeCell
        size={row.size}
        measure={row.measure}
        metric={metric}
        className="w-24 text-right text-xs font-mono text-zinc-500 flex-shrink-0 tabular-nums"
      />

      {onCopy && (
        <CopyButton row={row} target={copyTarget} disabled={copyDisabled} onCopy={onCopy} />
      )}
      <DeleteButton row={row} sizeLabel={sizeLabel} onDelete={onDelete} />
    </div>
  );
}

/**
 * `parent` is present only on results that span the tree, where a name
 * alone does not say where the thing lives. Clicking it navigates there.
 */
function RowName({
  row,
  onOpen,
  onReveal,
}: {
  row: Row;
  onOpen: (row: Row) => void;
  onReveal?: (row: Row) => void;
}) {
  return (
    <div className="flex-1 min-w-0">
      {row.kind === "dir" ? (
        <button
          className="w-full truncate text-left text-sm text-zinc-200 font-medium hover:text-accent-300 rounded focus-visible:outline-2 focus-visible:outline-accent-500"
          onClick={() => onOpen(row)}
        >
          {row.name}
        </button>
      ) : (
        <span className="block truncate text-sm text-zinc-400">{row.name}</span>
      )}
      {row.parent && (
        <button
          className="block w-full truncate text-left text-[10px] text-zinc-600 hover:text-accent-400 font-mono rounded focus-visible:outline-2 focus-visible:outline-accent-500"
          onClick={() => onReveal?.(row)}
          title={`Go to ${row.parent}`}
        >
          {row.parent}
        </button>
      )}
    </div>
  );
}

function CopyButton({
  row,
  target,
  disabled,
  onCopy,
}: {
  row: Row;
  target: string;
  disabled: boolean;
  onCopy: (row: Row) => void;
}) {
  return (
    <button
      className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100 ml-1 px-2 py-1 rounded-md
            bg-accent-600/15 border border-accent-500/30 text-accent-300
            hover:bg-accent-500/25 text-[10px] uppercase font-bold tracking-wider flex items-center gap-1
            cursor-pointer disabled:opacity-30 disabled:cursor-not-allowed focus-visible:outline-2 focus-visible:outline-accent-500"
      onClick={() => onCopy(row)}
      disabled={disabled}
      title={disabled ? "A copy is already running" : `Copy to ${target}`}
      aria-label={`Copy ${row.name} to ${target}`}
    >
      <IconDownload />
      <span className="hidden sm:inline">Copy</span>
    </button>
  );
}

function DeleteButton({
  row,
  sizeLabel,
  onDelete,
}: {
  row: Row;
  sizeLabel: string;
  onDelete: (row: Row) => void;
}) {
  return (
    // Reachable on focus as well as hover, so it exists for keyboard users.
    <button
      className="btn-nuke opacity-0 group-hover:opacity-100 focus-visible:opacity-100 ml-1 px-2 py-1 rounded-md
          bg-danger-600/20 border border-danger-500/30 text-danger-400
          hover:bg-danger-500/30 text-[10px] uppercase font-bold tracking-wider flex items-center gap-1
          cursor-pointer focus-visible:outline-2 focus-visible:outline-danger-500"
      onClick={() => onDelete(row)}
      aria-label={`Delete ${row.name}, ${sizeLabel}${
        row.kind === "dir" ? `, ${formatNumber(row.files)} files` : ""
      }`}
    >
      <IconTrash />
      <span className="hidden sm:inline">Delete</span>
    </button>
  );
}
