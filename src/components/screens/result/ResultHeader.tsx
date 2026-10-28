import type { ScanSession } from "../../../hooks/useScanSession";
import { nodeKey } from "../../../lib/size";
import { IconSearch } from "../../icons";

/** The strip above the results: scan state, breadcrumbs, search, rescan and disconnect. */
export function ResultHeader({ session, running }: { session: ScanSession; running: boolean }) {
  return (
    <header className="flex items-center gap-3 px-5 py-2.5 border-b border-zinc-800/80 shrink-0">
      <div
        className={`w-2 h-2 rounded-full shrink-0 ${
          running ? "bg-amber-500 animate-pulse" : "bg-emerald-500"
        }`}
        title={running ? "Scanning" : "Idle"}
      />

      <Breadcrumbs session={session} />

      {/* Search runs on the host over the loaded tree, so it is instant. */}
      <div className="relative shrink-0">
        <span className="absolute left-2 top-1/2 -translate-y-1/2 text-zinc-600">
          <IconSearch />
        </span>
        <input
          value={session.query}
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
  );
}

function Breadcrumbs({ session }: { session: ScanSession }) {
  const { crumbs, open } = session;
  return (
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
  );
}
