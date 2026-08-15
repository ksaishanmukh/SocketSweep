import { useEffect, useRef } from "react";

/**
 * The activity log, collapsed by default.
 *
 * Space belongs to the treemap; the log is there when something needs
 * explaining and out of the way otherwise.
 */
export function ConsoleDrawer({
  logs,
  open,
  onToggle,
}: {
  logs: string[];
  open: boolean;
  onToggle: () => void;
}) {
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open) endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs, open]);

  return (
    <div className="shrink-0 border-t border-zinc-800 bg-zinc-950">
      <button
        onClick={onToggle}
        className="w-full flex items-center gap-2 px-4 py-1.5 text-[11px] font-mono text-zinc-500
          hover:text-zinc-300 focus-visible:outline-2 focus-visible:outline-accent-500"
        aria-expanded={open}
        aria-controls="console-log"
      >
        <span className={`transition-transform ${open ? "rotate-90" : ""}`} aria-hidden>
          ›
        </span>
        Activity
        {logs.length > 0 && <span className="text-zinc-700">({logs.length})</span>}
      </button>

      {open && (
        <div
          id="console-log"
          className="h-32 px-4 pb-2 font-mono text-[11px] text-zinc-400 overflow-y-auto"
        >
          {logs.length === 0 ? (
            <p className="text-zinc-700">Nothing yet.</p>
          ) : (
            logs.map((log, i) => (
              <div key={i} className="mb-0.5 break-all">
                <span className="text-accent-500 mr-2" aria-hidden>
                  ❯
                </span>
                {log}
              </div>
            ))
          )}
          <div ref={endRef} />
        </div>
      )}
    </div>
  );
}
