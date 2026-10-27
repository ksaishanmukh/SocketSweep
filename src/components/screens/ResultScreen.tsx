import { useMemo } from "react";
import type { Row, View } from "../../api";
import { formatBytes, formatNumber } from "../../lib/format";
import { folderName } from "../../lib/path";
import { nodeKey, sizeOf } from "../../lib/size";
import { toItems } from "../../lib/treemapItems";
import { VIEW_PAGE_SIZE } from "../../hooks/useNavigation";
import type { ScanSession } from "../../hooks/useScanSession";
import { AppBreakdown } from "../AppBreakdown";
import { FileList } from "../FileList";
import { IconSearch } from "../icons";
import { MetricToggle } from "../MetricToggle";
import { ModeTabs } from "../ModeTabs";
import { ScanStatus } from "../ScanStatus";
import { SinceLastScan } from "../SinceLastScan";
import { SizeCell } from "../SizeCell";
import { Treemap } from "../Treemap";
import { TypeBreakdown } from "../TypeBreakdown";

export function ResultScreen({
  session,
  view,
  onDelete,
}: {
  session: ScanSession;
  /** The open folder; present whenever this screen is shown. */
  view: View;
  onDelete: (row: Row) => void;
}) {
  const {
    crumbs,
    totals,
    scanInfo,
    treemap,
    mode,
    metric,
    largest,
    types,
    apps,
    previous,
    query,
    searchResults,
    copy,
    open,
  } = session;
  const { items, counting } = useMemo(() => toItems(treemap, metric), [treemap, metric]);
  // A non-empty query owns the side panel even before its answer arrives, so
  // the folder listing is never shown under a search header.
  const searching = query.trim() !== "";
  const rows = searching ? (searchResults ?? []) : view.rows;
  const waiting = searching && searchResults === null;
  const running = scanInfo?.running ?? false;
  const copyDisabled = copy !== null;
  const copyTarget = session.copyDestination ? folderName(session.copyDestination) : undefined;
  const viewSize = sizeOf(view.size, metric);

  return (
    <div className="flex flex-col h-full animate-fade-in-up min-h-0">
      <header className="flex items-center gap-3 px-5 py-2.5 border-b border-zinc-800/80 shrink-0">
        <div
          className={`w-2 h-2 rounded-full shrink-0 ${
            running ? "bg-amber-500 animate-pulse" : "bg-emerald-500"
          }`}
          title={running ? "Scanning" : "Idle"}
        />

        <nav
          aria-label="Breadcrumb"
          className="flex items-center text-xs text-zinc-500 font-mono min-w-0 flex-1"
        >
          {crumbs.map((crumb, idx) => (
            <span key={nodeKey(crumb.node)} className="inline-flex items-center min-w-0">
              <button
                onClick={() => void open(crumb.node)}
                className={`hover:text-zinc-300 transition-colors truncate rounded px-0.5
                  focus-visible:outline-2 focus-visible:outline-accent-500 ${
                    idx === crumbs.length - 1 ? "text-zinc-300 font-semibold" : ""
                  }`}
                aria-current={idx === crumbs.length - 1 ? "page" : undefined}
              >
                {crumb.name}
              </button>
              {idx < crumbs.length - 1 && (
                <span className="mx-1 opacity-50 shrink-0" aria-hidden>
                  /
                </span>
              )}
            </span>
          ))}
        </nav>

        {/* Search runs on the host over the loaded tree, so it is instant. */}
        <div className="relative shrink-0">
          <span className="absolute left-2 top-1/2 -translate-y-1/2 text-zinc-600">
            <IconSearch />
          </span>
          <input
            value={query}
            onChange={(e) => session.setQuery(e.target.value)}
            placeholder="Search"
            aria-label="Search files and folders"
            className="w-44 bg-zinc-900 border border-zinc-800 rounded-lg pl-7 pr-2 py-1.5 text-xs text-zinc-200
              placeholder:text-zinc-600 focus-visible:outline-2 focus-visible:outline-accent-500"
          />
        </div>

        <button
          onClick={() => void session.scan()}
          className="px-3 py-1.5 rounded-lg text-xs font-medium bg-zinc-800 text-zinc-300 shrink-0
            hover:bg-zinc-700 hover:text-zinc-100 border border-zinc-700/50
            focus-visible:outline-2 focus-visible:outline-accent-500"
        >
          ↻ Rescan
        </button>
        <button
          onClick={() => void session.disconnect()}
          className="px-3 py-1.5 rounded-lg text-xs font-medium bg-zinc-800 text-zinc-400 shrink-0
            hover:bg-red-950/50 hover:text-red-400 border border-zinc-700/50
            focus-visible:outline-2 focus-visible:outline-accent-500"
        >
          Disconnect
        </button>
      </header>

      {/* Totals in one line, so the vertical space goes to the treemap. */}
      <div className="flex items-center gap-3 px-5 py-2 text-xs shrink-0 border-b border-zinc-800/60">
        {totals && (
          <>
            <span className="font-mono font-bold text-accent-400 tabular-nums text-sm">
              {running && <span className="counting-dot" aria-hidden />}
              {formatBytes(sizeOf(totals.size, metric))}
              {running && <span className="sr-only"> (still counting)</span>}
            </span>
            <span className="text-zinc-700">·</span>
            <span className="text-zinc-400 tabular-nums">{formatNumber(totals.files)} files</span>
            <span className="text-zinc-700">·</span>
            <span className="text-zinc-400 tabular-nums">{formatNumber(totals.dirs)} folders</span>
            {totals.errors > 0 && (
              <>
                <span className="text-zinc-700">·</span>
                <span
                  className="text-amber-500/80 tabular-nums"
                  title="Folders that could not be read and files that could not be measured"
                >
                  {formatNumber(totals.errors)} unreadable
                </span>
              </>
            )}
            {scanInfo && !running && (
              <>
                <span className="text-zinc-700">·</span>
                <span className="text-zinc-500 tabular-nums">
                  {(scanInfo.elapsedMs / 1000).toFixed(1)}s
                </span>
              </>
            )}
            <SinceLastScan
              previous={previous}
              totals={totals}
              metric={metric}
              incomplete={scanInfo?.outcome !== "complete"}
            />
          </>
        )}

        <div className="flex-1" />
        <MetricToggle metric={metric} onChange={session.setMetric} />
        <ModeTabs mode={mode} onChange={session.setMode} />
      </div>

      {scanInfo && (
        <ScanStatus
          scan={scanInfo}
          onStop={session.cancelScan}
          onRescan={() => void session.scan()}
        />
      )}

      {/* The treemap is the point of the screen, so it gets the room. */}
      <div className="flex-1 min-h-0 flex flex-col lg:flex-row gap-3 p-3">
        <div className="flex-1 min-h-0 glass-card rounded-xl overflow-hidden">
          {mode === "treemap" && (
            <Treemap
              items={items}
              counting={counting}
              emptyMessage={
                running && view.measure !== "final"
                  ? "Measuring this folder…"
                  : "Nothing to show here."
              }
              onOpen={(id, isDir) => {
                if (isDir && treemap) void open({ session: treemap.node.session, id });
              }}
            />
          )}

          {/* Biggest files anywhere on the device, regardless of depth. */}
          {mode === "largest" && (
            <FileList
              rows={largest}
              totalSize={largest[0] ? sizeOf(largest[0].size, metric) : 0}
              metric={metric}
              onOpen={(row) => void open(row.node)}
              onDelete={onDelete}
              onCopy={(row) => void session.startCopy(row)}
              copyDisabled={copyDisabled}
              copyTarget={copyTarget}
              onReveal={(row) => void session.reveal(row)}
              emptyMessage={running ? "Looking for files…" : "No files found."}
            />
          )}

          {mode === "types" && <TypeBreakdown groups={types} metric={metric} />}

          {mode === "apps" && (
            <AppBreakdown
              apps={apps}
              metric={metric}
              onOpen={(node) => {
                // The app's folder is shown in the treemap, not in this list.
                session.setMode("treemap");
                void open(node);
              }}
            />
          )}
        </div>

        <div className="w-full lg:w-[26rem] shrink-0 flex flex-col min-h-0 glass-card rounded-xl">
          <div className="flex items-center gap-2 px-4 py-2 border-b border-zinc-800/60 shrink-0">
            <span className="text-[11px] font-semibold text-zinc-500 uppercase tracking-wider flex-1 truncate">
              {waiting
                ? `Searching for “${query.trim()}”…`
                : searching
                  ? `${rows.length} matches for “${query.trim()}”`
                  : view.path}
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

          {/*
            Search takes over this panel only, leaving the main canvas as the
            context you are searching within.
          */}
          <FileList
            key={searching ? "search" : nodeKey(view.node)}
            rows={rows}
            totalSize={searching ? (rows[0] ? sizeOf(rows[0].size, metric) : 0) : viewSize}
            metric={metric}
            onOpen={(row) => void open(row.node)}
            onDelete={onDelete}
            onCopy={(row) => void session.startCopy(row)}
            copyDisabled={copyDisabled}
            copyTarget={copyTarget}
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
            <div className="flex items-center gap-2 px-4 py-2 border-t border-zinc-800/60 shrink-0">
              <span className="text-[11px] text-zinc-600 flex-1 tabular-nums">
                Showing {formatNumber(view.rows.length)} of{" "}
                {formatNumber(view.rows.length + view.hidden)}
              </span>
              <button
                onClick={() => void session.loadMore()}
                disabled={session.loadingMore}
                className="px-2.5 py-1 rounded-md text-[11px] font-medium bg-zinc-800 text-zinc-300 border border-zinc-700/50
                  hover:bg-zinc-700 hover:text-zinc-100 disabled:opacity-50 disabled:cursor-wait
                  focus-visible:outline-2 focus-visible:outline-accent-500"
              >
                {session.loadingMore
                  ? "Loading…"
                  : `Load ${formatNumber(Math.min(VIEW_PAGE_SIZE, view.hidden))} more`}
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
