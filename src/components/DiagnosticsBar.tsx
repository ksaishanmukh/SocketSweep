import { useDiagnostics } from "../hooks/useDiagnostics";

/** Bug-report helpers in the console drawer: copy a diagnostics report, open the log folder. */
export function DiagnosticsBar() {
  const { includePrivate, setIncludePrivate, status, problem, copy, openFolder } = useDiagnostics();
  const link = "text-accent hover:underline";

  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 px-4 pb-1 font-mono text-[11px] text-muted">
      <button className={link} onClick={() => void copy()}>
        Copy diagnostics
      </button>
      <label className="flex items-center gap-1.5">
        <input
          type="checkbox"
          checked={includePrivate}
          onChange={(e) => setIncludePrivate(e.target.checked)}
        />
        Include device serial and paths
      </label>
      <button className={link} onClick={() => void openFolder()}>
        Open log folder
      </button>
      <span role="status" className={status === "failed" ? "text-danger" : "text-faint"}>
        {status === "copied" && "Copied to the clipboard."}
        {status === "failed" && (problem ?? "Failed.")}
      </span>
    </div>
  );
}
