import { useCallback, useState } from "react";
import { errorMessage } from "../api";
import type { Notify } from "./types";

const MAX_LOG_LINES = 200;

/** The console's log lines, and `fail` for reporting a failed command to the user and the log. */
export function useSessionLog(notify: Notify) {
  const [logs, setLogs] = useState<string[]>([]);

  const log = useCallback((msg: string) => {
    setLogs((prev) =>
      [...prev, `[${new Date().toLocaleTimeString()}] ${msg}`].slice(-MAX_LOG_LINES),
    );
  }, []);

  const fail = useCallback(
    (err: unknown) => {
      const message = errorMessage(err);
      notify(message, "error");
      log(`[ERROR] ${message}`);
      return message;
    },
    [log, notify],
  );

  return { logs, log, fail };
}
