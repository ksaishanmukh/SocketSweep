import { useRef } from "react";
import { useVirtualizer, type VirtualItem } from "@tanstack/react-virtual";
import type { Row } from "../api";
import { nodeKey } from "../lib/size";
import { FileRow, type FileRowActions } from "./FileRow";

const ROW_HEIGHT = 34;
/** Cross-tree results carry a second line showing the containing folder. */
const ROW_HEIGHT_WITH_PARENT = 44;

export interface FileListProps extends FileRowActions {
  rows: Row[];
  /** What a full bar means, in the chosen metric. */
  totalSize: number;
  emptyMessage?: string;
}

/**
 * Windowed list of directory entries.
 *
 * Only visible rows are mounted. A single folder can hold tens of thousands of
 * entries, so render cost has to stay independent of how large it is.
 */
export function FileList({
  rows,
  totalSize,
  emptyMessage = "This folder is empty.",
  ...actions
}: FileListProps) {
  const scrollRef = useRef<HTMLDivElement>(null);

  // The rule warns that this API is not React Compiler memoization-safe. This
  // project does not use the compiler, and a permanent warning only teaches
  // people to ignore warnings.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => (rows[i]?.parent ? ROW_HEIGHT_WITH_PARENT : ROW_HEIGHT),
    getItemKey: (i) => (rows[i] ? nodeKey(rows[i].node) : i),
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
        {virtualizer.getVirtualItems().map((item) => (
          <PositionedRow
            key={nodeKey(rows[item.index].node)}
            item={item}
            row={rows[item.index]}
            totalSize={totalSize}
            actions={actions}
          />
        ))}
      </div>
    </div>
  );
}

function PositionedRow({
  item,
  row,
  totalSize,
  actions,
}: {
  item: VirtualItem;
  row: Row;
  totalSize: number;
  actions: FileRowActions;
}) {
  return (
    <div
      role="listitem"
      style={{
        position: "absolute",
        top: 0,
        left: 0,
        width: "100%",
        height: item.size,
        transform: `translateY(${item.start}px)`,
      }}
    >
      <FileRow row={row} totalSize={totalSize} {...actions} />
    </div>
  );
}
