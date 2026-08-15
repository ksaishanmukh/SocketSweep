import { useCallback, useEffect, useRef, useState } from "react";

export interface Toast {
  id: number;
  message: string;
  type: "error" | "success" | "info";
  exiting?: boolean;
}

const LIFETIME_MS = 5000;
/** Matches the CSS exit animation so the node is removed once it is invisible. */
const EXIT_MS = 300;

let nextId = 0;

/**
 * Transient notifications.
 *
 * Errors stay until dismissed; everything else expires on a timer. An error is
 * the one kind of message a reader may need twice, so it does not get to
 * disappear on its own.
 */
export function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>());

  const remove = useCallback((id: number) => {
    setToasts((prev) => prev.map((t) => (t.id === id ? { ...t, exiting: true } : t)));
    const timer = setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), EXIT_MS);
    timers.current.set(-id, timer);
  }, []);

  const push = useCallback(
    (message: string, type: Toast["type"] = "error") => {
      const id = ++nextId;
      setToasts((prev) => [...prev, { id, message, type }]);

      if (type !== "error") {
        timers.current.set(
          id,
          setTimeout(() => remove(id), LIFETIME_MS),
        );
      }
      return id;
    },
    [remove],
  );

  const dismiss = useCallback(
    (id: number) => {
      const timer = timers.current.get(id);
      if (timer) {
        clearTimeout(timer);
        timers.current.delete(id);
      }
      remove(id);
    },
    [remove],
  );

  useEffect(() => {
    const pending = timers.current;
    return () => pending.forEach(clearTimeout);
  }, []);

  return { toasts, push, dismiss };
}
