import { useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { formatBytes, formatNumber } from "../lib/format";
import type { Row } from "../lib/types";
import { IconFile, IconFolder, IconTrash } from "./icons";
import { SizeBar } from "./SizeBar";

const ROW_HEIGHT = 34;
/** Cross-tree results carry a second line showing the containing folder. */
const ROW_HEIGHT_WITH_PARENT = 44;

/**
 * Windowed list of directory entries.
 *
 * Only visible rows are mounted. A single folder can hold tens of thousands of
 * entries, so render cost has to stay independent of how large it is.
 */
export function FileList({
  rows,
  totalSize,
  onOpen,
  onDelete,
  onReveal,
  showBar = true,
  emptyMessage = "This folder is empty.",
}: {
  rows: Row[];
  totalSize: number;
  onOpen: (row: Row) => void;
  onDelete: (row: Row) => void;
  /** Navigate to a row's containing folder. Only meaningful for cross-tree results. */
  onReveal?: (row: Row) => void;
  /**
   * The bar costs 96px. In the narrow side panel that is the difference
   * between reading "Documents" and reading "Doc…", and the treemap beside it
   * already shows proportion.
   */
  showBar?: boolean;
  emptyMessage?: string;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);

  // The rule warns that this API is not React Compiler memoization-safe. This
  // project does not use the compiler, and a permanent warning only teaches
  // people to ignore warnings.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => (rows[i]?.parent ? ROW_HEIGHT_WITH_PARENT : ROW_HEIGHT),
    overscan: 12,
  });

  if (rows.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center text-sm text-zinc-600">
        {emptyMessage}
      </div>
    );
  }

  return (
    <div ref={scrollRef} className="flex-1 overflow-y-auto min-h-0 px-3 pb-3">
      <div
        role="list"
        style={{ height: virtualizer.getTotalSize(), position: "relative", width: "100%" }}
      >
        {virtualizer.getVirtualItems().map((v) => {
          const row = rows[v.index];
          return (
            <div
              key={row.id}
              role="listitem"
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                height: v.size,
                transform: `translateY(${v.start}px)`,
              }}
            >
              <FileRow
                row={row}
                totalSize={totalSize}
                onOpen={onOpen}
                onDelete={onDelete}
                onReveal={onReveal}
                showBar={showBar}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}

function FileRow({
  row,
  totalSize,
  onOpen,
  onDelete,
  onReveal,
  showBar,
}: {
  row: Row;
  totalSize: number;
  onOpen: (row: Row) => void;
  onDelete: (row: Row) => void;
  onReveal?: (row: Row) => void;
  showBar: boolean;
}) {
  const ratio = totalSize > 0 ? row.size / totalSize : 0;

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
      <span className={row.isDir ? "text-accent-400" : "text-zinc-500"}>
        {row.isDir ? <IconFolder /> : <IconFile />}
      </span>

      {/*
        `parent` is present only on results that span the tree, where a name
        alone does not say where the thing lives. Clicking it navigates there.
      */}
      <div className="flex-1 min-w-0">
        {row.isDir ? (
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

      {!row.complete && (
        <span
          className="text-[10px] text-zinc-600"
          title="Still being scanned"
          aria-label="Still being scanned"
        >
          …
        </span>
      )}

      {showBar && <SizeBar ratio={ratio} />}

      <span className="w-20 text-right text-xs font-mono text-zinc-500 flex-shrink-0 tabular-nums">
        {formatBytes(row.size)}
      </span>

      {/* Reachable on focus as well as hover, so it exists for keyboard users. */}
      <button
        className="btn-nuke opacity-0 group-hover:opacity-100 focus-visible:opacity-100 ml-1 px-2 py-1 rounded-md
          bg-danger-600/20 border border-danger-500/30 text-danger-400
          hover:bg-danger-500/30 text-[10px] uppercase font-bold tracking-wider flex items-center gap-1
          cursor-pointer focus-visible:outline-2 focus-visible:outline-danger-500"
        onClick={() => onDelete(row)}
        aria-label={`Delete ${row.name}, ${formatBytes(row.size)}${
          row.isDir ? `, ${formatNumber(row.files)} files` : ""
        }`}
      >
        <IconTrash />
        <span className="hidden sm:inline">Delete</span>
      </button>
    </div>
  );
}
