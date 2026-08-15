import { useMemo } from "react";
import { formatBytes, formatNumber } from "../../lib/format";
import type { TreemapItem } from "../../lib/squarify";
import type { Mode } from "../../hooks/useScanSession";
import type {
  AppUsage,
  Crumb,
  Row,
  ScanRecord,
  Stats,
  TreemapNode,
  TypeGroup,
  View,
} from "../../lib/types";
import { AppBreakdown } from "../AppBreakdown";
import { FileList } from "../FileList";
import { IconSearch } from "../icons";
import { ModeTabs } from "../ModeTabs";
import { SinceLastScan } from "../SinceLastScan";
import { Treemap } from "../Treemap";
import { TypeBreakdown } from "../TypeBreakdown";

function toItems(node: TreemapNode | null): TreemapItem[] {
  if (!node) return [];
  const map = (n: TreemapNode): TreemapItem => ({
    id: n.id,
    name: n.name,
    size: n.size,
    isDir: n.isDir,
    children: n.children.length ? n.children.map(map) : undefined,
  });
  return node.children.map(map);
}

export function ResultScreen({
  view,
  crumbs,
  stats,
  treemap,
  mode,
  largest,
  types,
  apps,
  previous,
  onModeChange,
  onReveal,
  elapsedMs,
  query,
  searchResults,
  onQueryChange,
  onOpen,
  onRescan,
  onDisconnect,
  onDelete,
}: {
  view: View;
  crumbs: Crumb[];
  stats: Stats;
  treemap: TreemapNode | null;
  mode: Mode;
  largest: Row[];
  types: TypeGroup[];
  apps: AppUsage[];
  previous: ScanRecord | null;
  onModeChange: (m: Mode) => void;
  onReveal: (row: Row) => void;
  elapsedMs: number | null;
  query: string;
  searchResults: Row[] | null;
  onQueryChange: (q: string) => void;
  onOpen: (id: number) => void;
  onRescan: () => void;
  onDisconnect: () => void;
  onDelete: (row: Row) => void;
}) {
  const items = useMemo(() => toItems(treemap), [treemap]);
  const searching = searchResults !== null;
  const rows = searchResults ?? view.rows;

  return (
    <div className="flex flex-col h-full animate-fade-in-up min-h-0">
      <header className="flex items-center gap-3 px-5 py-2.5 border-b border-zinc-800/80 shrink-0">
        <div
          className={`w-2 h-2 rounded-full shrink-0 ${
            stats.scanning ? "bg-amber-500 animate-pulse" : "bg-emerald-500"
          }`}
          title={stats.scanning ? "Scanning" : "Idle"}
        />

        <nav
          aria-label="Breadcrumb"
          className="flex items-center text-xs text-zinc-500 font-mono min-w-0 flex-1"
        >
          {crumbs.map((crumb, idx) => (
            <span key={crumb.id} className="inline-flex items-center min-w-0">
              <button
                onClick={() => onOpen(crumb.id)}
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

        {/*
          Search over the loaded tree. It is a Rust query over a flat array, so
          it is instant — and it answers the question the old UI could only
          answer by expanding folders one at a time.
        */}
        <div className="relative shrink-0">
          <span className="absolute left-2 top-1/2 -translate-y-1/2 text-zinc-600">
            <IconSearch />
          </span>
          <input
            value={query}
            onChange={(e) => onQueryChange(e.target.value)}
            placeholder="Search"
            aria-label="Search files and folders"
            className="w-44 bg-zinc-900 border border-zinc-800 rounded-lg pl-7 pr-2 py-1.5 text-xs text-zinc-200
              placeholder:text-zinc-600 focus-visible:outline-2 focus-visible:outline-accent-500"
          />
        </div>

        <button
          onClick={onRescan}
          className="px-3 py-1.5 rounded-lg text-xs font-medium bg-zinc-800 text-zinc-300 shrink-0
            hover:bg-zinc-700 hover:text-zinc-100 border border-zinc-700/50
            focus-visible:outline-2 focus-visible:outline-accent-500"
        >
          ↻ Rescan
        </button>
        <button
          onClick={onDisconnect}
          className="px-3 py-1.5 rounded-lg text-xs font-medium bg-zinc-800 text-zinc-400 shrink-0
            hover:bg-red-950/50 hover:text-red-400 border border-zinc-700/50
            focus-visible:outline-2 focus-visible:outline-accent-500"
        >
          Disconnect
        </button>
      </header>

      {/*
        One compact strip. Four cards of oversized type used to occupy ~110px
        above a 192px treemap; the proportions were backwards.
      */}
      <div className="flex items-center gap-3 px-5 py-2 text-xs shrink-0 border-b border-zinc-800/60">
        <span className="font-mono font-bold text-accent-400 tabular-nums text-sm">
          {formatBytes(stats.size)}
        </span>
        <span className="text-zinc-700">·</span>
        <span className="text-zinc-400 tabular-nums">{formatNumber(stats.files)} files</span>
        <span className="text-zinc-700">·</span>
        <span className="text-zinc-400 tabular-nums">{formatNumber(stats.dirs)} folders</span>
        {elapsedMs !== null && (
          <>
            <span className="text-zinc-700">·</span>
            <span className="text-zinc-500 tabular-nums">{(elapsedMs / 1000).toFixed(1)}s</span>
          </>
        )}
        <SinceLastScan previous={previous} stats={stats} />

        <div className="flex-1" />
        <ModeTabs mode={mode} onChange={onModeChange} />
      </div>

      {/*
        The treemap is the product. It gets the space now — it was a 192px strip
        under a stack of stat cards, with a console log pinned below it.
      */}
      <div className="flex-1 min-h-0 flex flex-col lg:flex-row gap-3 p-3">
        <div className="flex-1 min-h-0 glass-card rounded-xl overflow-hidden">
          {mode === "treemap" && (
            <Treemap items={items} onOpen={(id, isDir) => isDir && onOpen(id)} />
          )}

          {/*
            The question this app exists to answer. Until now it could only be
            answered by expanding folders one at a time.
          */}
          {mode === "largest" && (
            <FileList
              rows={largest}
              totalSize={largest[0]?.size ?? 0}
              onOpen={(row) => onOpen(row.id)}
              onDelete={onDelete}
              onReveal={onReveal}
              emptyMessage="No files found."
            />
          )}

          {mode === "types" && <TypeBreakdown groups={types} />}

          {mode === "apps" && <AppBreakdown apps={apps} onOpen={onOpen} />}
        </div>

        <div className="w-full lg:w-[26rem] shrink-0 flex flex-col min-h-0 glass-card rounded-xl">
          <div className="flex items-center gap-2 px-4 py-2 border-b border-zinc-800/60 shrink-0">
            <span className="text-[11px] font-semibold text-zinc-500 uppercase tracking-wider flex-1 truncate">
              {searching ? `${rows.length} matches for “${query}”` : view.path}
            </span>
            {!searching && view.hidden > 0 && (
              <span className="text-[11px] text-zinc-600">+{formatNumber(view.hidden)} more</span>
            )}
          </div>

          {/*
            Search takes over this panel only. It used to blank the main canvas,
            which threw away the context you were searching within.
          */}
          <FileList
            rows={rows}
            totalSize={searching ? (rows[0]?.size ?? 0) : view.size}
            onOpen={(row) => onOpen(row.id)}
            onDelete={onDelete}
            onReveal={searching ? onReveal : undefined}
            emptyMessage={searching ? "No matches." : "This folder is empty."}
          />
        </div>
      </div>
    </div>
  );
}
