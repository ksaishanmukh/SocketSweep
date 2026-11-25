import { useRef, type ReactNode } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { Row } from "../../api";
import { useListKeys, type ListKeyActions } from "../../hooks/useListKeys";
import { nodeKey } from "../../lib/size";
import { ROW_HEIGHT } from "./columns";
import type { Hover, SetHover } from "./hover";
import { ListRow, type RowActions } from "./ListRow";

/**
 * The rows, windowed: only the visible ones are mounted, since one folder can
 * hold tens of thousands of entries and render cost must not grow with it.
 */
export function ListBody({
  rows,
  shareOf,
  isPicked,
  hover,
  onHover,
  actions,
  keys,
  resetKey,
  footer,
}: {
  rows: Row[];
  /** A row's part of the open folder, or null when shares do not apply. */
  shareOf: (row: Row) => number | null;
  isPicked: (row: Row) => boolean;
  hover: Hover | null;
  onHover: SetHover;
  actions: RowActions;
  keys: ListKeyActions;
  /** Another folder: the active row starts over at the first. */
  resetKey: string;
  /** Below the last row, e.g. "Load more". */
  footer?: ReactNode;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);

  // The rule warns that this API is not React Compiler memoization-safe. This
  // project does not use the compiler, and a permanent warning only teaches
  // people to ignore warnings.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    getItemKey: (i) => (rows[i] ? nodeKey(rows[i].node) : i),
    overscan: 12,
  });
  const nav = useListKeys({
    rows,
    resetKey,
    listRef: scrollRef,
    scrollTo: (i) => virtualizer.scrollToIndex(i),
    pageSize: () => Math.floor((scrollRef.current?.clientHeight ?? 0) / ROW_HEIGHT),
    isPicked,
    open: actions.onOpen,
    keys,
  });

  return (
    <div
      ref={scrollRef}
      onKeyDown={nav.onKeyDown}
      className="flex-1 min-h-0 overflow-y-auto px-1 pb-2"
    >
      <div role="rowgroup" style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((item) => {
          const row = rows[item.index];
          const key = nodeKey(row.node);
          return (
            <div
              key={key}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                height: item.size,
                transform: `translateY(${item.start}px)`,
              }}
            >
              <ListRow
                row={row}
                index={item.index}
                active={item.index === nav.active}
                onActivate={() => nav.activate(item.index)}
                share={shareOf(row)}
                picked={isPicked(row)}
                highlighted={hover?.key === key && hover.from === "map"}
                onHoverChange={(on) => onHover(on ? { key, from: "list" } : null)}
                actions={actions}
              />
            </div>
          );
        })}
      </div>
      {footer}
    </div>
  );
}
