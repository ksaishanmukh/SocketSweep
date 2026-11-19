import { useEffect, useRef } from "react";
import { DiagnosticsBar } from "./DiagnosticsBar";

/**
 * The activity log, collapsed by default.
 *
 * Space belongs to the treemap; the log is there when something needs
 * explaining and out of the way otherwise.
 */
export function ConsoleDrawer({
  logs,
  version,
  open,
  onToggle,
}: {
  logs: string[];
  version: string | null;
  open: boolean;
  onToggle: () => void;
}) {
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open) endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs, open]);

  return (
    <div className="shrink-0 border-t border-line bg-panel">
      <div className="flex items-center">
        <button
          onClick={onToggle}
          className="flex-1 flex items-center gap-2 px-4 py-1.5 text-[11px] font-mono text-muted
          hover:text-fg"
          aria-expanded={open}
          aria-controls="console-log"
        >
          <span className={`transition-transform ${open ? "rotate-90" : ""}`} aria-hidden>
            ›
          </span>
          Activity
          {logs.length > 0 && <span className="text-faint">({logs.length})</span>}
        </button>
        {version && <span className="px-4 text-[11px] font-mono text-faint">v{version}</span>}
      </div>

      {open && <DiagnosticsBar />}

      {open && (
        <div
          id="console-log"
          className="h-32 px-4 pb-2 font-mono text-[11px] text-muted overflow-y-auto"
        >
          {logs.length === 0 ? (
            <p className="text-faint">Nothing yet.</p>
          ) : (
            logs.map((log, i) => (
              <div key={i} className="mb-0.5 break-all">
                <span className="text-accent mr-2" aria-hidden>
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
