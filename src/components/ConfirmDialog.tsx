import { useEffect, useRef, useState } from "react";
import { formatBytes, formatNumber } from "../lib/format";
import type { Row } from "../lib/types";
import { IconTrash } from "./icons";

/** Above this, the dialog asks the user to type the name rather than just click. */
const TYPE_TO_CONFIRM_BYTES = 1024 ** 3;
const TYPE_TO_CONFIRM_FILES = 100;

/**
 * Deleting is the one irreversible thing this app does, and the dialog it
 * replaces showed only a name — no indication of what was about to be lost.
 * The host already knows the subtree totals, and that is the information that
 * actually prevents mistakes.
 */
export function ConfirmDialog({
  target,
  onCancel,
  onConfirm,
}: {
  target: Row;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const [typed, setTyped] = useState("");
  const cancelRef = useRef<HTMLButtonElement>(null);

  const heavy = target.size >= TYPE_TO_CONFIRM_BYTES || target.files >= TYPE_TO_CONFIRM_FILES;
  const armed = !heavy || typed === target.name;

  useEffect(() => {
    // Focus starts on Cancel: Enter should not delete anything by reflex.
    cancelRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 backdrop-blur-sm animate-fade-in-up"
      role="dialog"
      aria-modal="true"
      aria-labelledby="confirm-title"
    >
      <div className="bg-zinc-900 border border-zinc-800 rounded-2xl p-6 max-w-sm w-full shadow-2xl">
        <div className="flex items-center gap-3 mb-4">
          <div className="w-10 h-10 rounded-full bg-red-500/10 flex items-center justify-center text-red-500">
            <IconTrash />
          </div>
          <h3 id="confirm-title" className="text-lg font-bold text-zinc-100">
            Delete permanently?
          </h3>
        </div>

        <p className="text-sm font-mono text-zinc-300 bg-zinc-950 p-2 rounded-lg break-all border border-zinc-800 mb-3">
          {target.name}
        </p>

        <p className="text-sm text-zinc-400 mb-4">
          This frees <span className="font-semibold text-zinc-200">{formatBytes(target.size)}</span>
          {target.isDir && (
            <>
              {" "}
              across{" "}
              <span className="font-semibold text-zinc-200">
                {formatNumber(target.files)} files
              </span>
            </>
          )}
          . It cannot be undone.
        </p>

        {heavy && (
          <label className="block mb-5">
            <span className="text-xs text-zinc-500">
              Type <span className="font-mono text-zinc-300">{target.name}</span> to confirm
            </span>
            <input
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              autoComplete="off"
              spellCheck={false}
              className="mt-1 w-full bg-zinc-950 border border-zinc-800 rounded-lg px-2 py-1.5 text-sm font-mono text-zinc-200
                focus-visible:outline-2 focus-visible:outline-accent-500"
            />
          </label>
        )}

        <div className="flex items-center gap-3 justify-end">
          <button
            ref={cancelRef}
            onClick={onCancel}
            className="px-4 py-2 rounded-lg text-sm font-medium text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800
              transition-colors rounded focus-visible:outline-2 focus-visible:outline-accent-500"
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            disabled={!armed}
            className="px-4 py-2 rounded-lg text-sm font-bold bg-red-600 hover:bg-red-500 text-white transition-colors
              shadow-lg shadow-red-500/20 disabled:opacity-40 disabled:cursor-not-allowed disabled:shadow-none
              focus-visible:outline-2 focus-visible:outline-red-400"
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  );
}
