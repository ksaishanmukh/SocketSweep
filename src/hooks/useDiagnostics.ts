import { useCallback, useState } from "react";
import { appLogDir } from "@tauri-apps/api/path";
import { openPath } from "@tauri-apps/plugin-opener";
import { commands, errorMessage } from "../api";

/** What the last button press did, for the line shown beside the buttons. */
export type DiagnosticsStatus = "idle" | "copied" | "failed";

/**
 * "Copy diagnostics" and "Open log folder".
 *
 * The report is built by the host, which alone knows the phone and the log
 * file; this only asks for it with or without the private details and puts
 * it on the clipboard.
 */
export function useDiagnostics() {
  const [includePrivate, setIncludePrivate] = useState(false);
  const [status, setStatus] = useState<DiagnosticsStatus>("idle");
  const [problem, setProblem] = useState<string | null>(null);

  const fail = useCallback((err: unknown) => {
    setProblem(errorMessage(err));
    setStatus("failed");
  }, []);

  const copy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(await commands.diagnostics(includePrivate));
      setStatus("copied");
    } catch (err) {
      fail(err);
    }
  }, [includePrivate, fail]);

  const openFolder = useCallback(async () => {
    try {
      await openPath(await appLogDir());
      setStatus("idle");
    } catch (err) {
      fail(err);
    }
  }, [fail]);

  return { includePrivate, setIncludePrivate, status, problem, copy, openFolder };
}
