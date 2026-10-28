import type { Row, View } from "../../../api";
import { VIEW_PAGE_SIZE } from "../../../hooks/useNavigation";
import type { ScanSession } from "../../../hooks/useScanSession";
import { formatNumber } from "../../../lib/format";
import { nodeKey, sizeOf } from "../../../lib/size";
import { FileList } from "../../FileList";
import { SizeCell } from "../../SizeCell";
import { copyOptions } from "./copyOptions";

/** The open folder's rows, or the search results that take its place. */
export function SidePanel({
  session,
  view,
  onDelete,
}: {
  session: ScanSession;
  view: View;
  onDelete: (row: Row) => void;
}) {
  const { query, searchResults, metric, open } = session;
  // A non-empty query owns the side panel even before its answer arrives, so
  // the folder listing is never shown under a search header.
  const searching = query.trim() !== "";
  const rows = searching ? (searchResults ?? []) : view.rows;
  const waiting = searching && searchResults === null;
  const viewSize = sizeOf(view.size, metric);

  return (
    <div className="w-full lg:w-[26rem] shrink-0 flex flex-col min-h-0 glass-card rounded-xl">
      <PanelHeader
        view={view}
        metric={metric}
        title={
          waiting
            ? `Searching for “${query.trim()}”…`
            : searching
              ? `${rows.length} matches for “${query.trim()}”`
              : view.path
        }
        searching={searching}
      />

      {/*
        Search takes over this panel only; the main canvas stays as context.
      */}
      <FileList
        key={searching ? "search" : nodeKey(view.node)}
        rows={rows}
        totalSize={searching ? (rows[0] ? sizeOf(rows[0].size, metric) : 0) : viewSize}
        metric={metric}
        onOpen={(row) => void open(row.node)}
        onDelete={onDelete}
        {...copyOptions(session)}
        onReveal={searching ? (row) => void session.reveal(row) : undefined}
        showBar={false}
        emptyMessage={
          waiting
            ? "Searching…"
            : searching
              ? "No matches."
              : view.measure === "unknown"
                ? "Listing this folder…"
                : "This folder is empty."
        }
      />

      {/* Large folders arrive in pages, so say how much is not on screen yet. */}
      {!searching && view.hidden > 0 && (
        <LoadMore view={view} loading={session.loadingMore} onLoadMore={session.loadMore} />
      )}
    </div>
  );
}

function PanelHeader({
  view,
  metric,
  title,
  searching,
}: {
  view: View;
  metric: ScanSession["metric"];
  title: string;
  searching: boolean;
}) {
  return (
    <div className="flex items-center gap-2 px-4 py-2 border-b border-zinc-800/60 shrink-0">
      <span className="text-[11px] font-semibold text-zinc-500 uppercase tracking-wider flex-1 truncate">
        {title}
      </span>
      {!searching && (
        <SizeCell
          size={view.size}
          measure={view.measure}
          metric={metric}
          className="text-[11px] font-mono text-zinc-500 tabular-nums shrink-0"
        />
      )}
      {!searching && view.unreadable && (
        <span
          className="text-[11px] text-amber-500/80"
          title="Some folders in here could not be read, so sizes are low"
          aria-label="Some folders could not be read"
        >
          ⚠
        </span>
      )}
    </div>
  );
}

function LoadMore({
  view,
  loading,
  onLoadMore,
}: {
  view: View;
  loading: boolean;
  onLoadMore: () => Promise<void>;
}) {
  return (
    <div className="flex items-center gap-2 px-4 py-2 border-t border-zinc-800/60 shrink-0">
      <span className="text-[11px] text-zinc-600 flex-1 tabular-nums">
        Showing {formatNumber(view.rows.length)} of {formatNumber(view.rows.length + view.hidden)}
      </span>
      <button
        onClick={() => void onLoadMore()}
        disabled={loading}
        className="px-2.5 py-1 rounded-md text-[11px] font-medium bg-zinc-800 text-zinc-300 border border-zinc-700/50
                  hover:bg-zinc-700 hover:text-zinc-100 disabled:opacity-50 disabled:cursor-wait
                  focus-visible:outline-2 focus-visible:outline-accent-500"
      >
        {loading ? "Loading…" : `Load ${formatNumber(Math.min(VIEW_PAGE_SIZE, view.hidden))} more`}
      </button>
    </div>
  );
}
