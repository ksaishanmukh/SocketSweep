import { useEffect, useRef, useState } from "react";
import { formatBytes, formatNumber } from "../lib/format";
import type { Row } from "../lib/types";
import { IconTrash } from "./icons";

/** Above this, the dialog asks the user to type the name rather than just click. */
const TYPE_TO_CONFIRM_BYTES = 1024 ** 3;
const TYPE_TO_CONFIRM_FILES = 100;

/** A row the scanner has not read yet reports 0 bytes, which says nothing about what is there. */
type Target = Row & { listed?: boolean };

const FOCUSABLE =
  'button:not([disabled]), input:not([disabled]), [href], [tabindex]:not([tabindex="-1"])';

/** Joins a directory and a name without doubling the separator. */
function joinPath(dir: string, name: string): string {
  return dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`;
}

/**
 * Deleting is the one irreversible thing this app does.
 *
 * The dialog leads with what will be lost — size, and file count for a folder —
 * because that is the information that prevents mistakes, and the host already
 * knows it. It traps focus while open and hands focus back to whatever opened it.
 */
export function ConfirmDialog({
  target,
  dir,
  onCancel,
  onConfirm,
}: {
  target: Target;
  /** Folder containing the target, when known. */
  dir?: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const [typed, setTyped] = useState("");
  const cancelRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const onCancelRef = useRef(onCancel);
  useEffect(() => {
    onCancelRef.current = onCancel;
  });

  const unscanned = target.listed === false;
  // Lossy names cannot be retyped, so they fall back to a fixed word.
  const untypeable = target.name.includes("\uFFFD") || target.name.trim() === "";
  const phrase = untypeable ? "DELETE" : target.name;
  const heavy =
    unscanned || target.size >= TYPE_TO_CONFIRM_BYTES || target.files >= TYPE_TO_CONFIRM_FILES;
  const armed = !heavy || typed.normalize("NFC") === phrase.normalize("NFC");

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    // Focus starts on Cancel, so Enter cannot delete anything by reflex.
    cancelRef.current?.focus();

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onCancelRef.current();
        return;
      }
      if (e.key !== "Tab") return;
      const nodes = dialogRef.current?.querySelectorAll<HTMLElement>(FOCUSABLE);
      if (!nodes || nodes.length === 0) return;
      const first = nodes[0];
      const last = nodes[nodes.length - 1];
      const active = document.activeElement;
      if (!dialogRef.current?.contains(active)) {
        e.preventDefault();
        first.focus();
      } else if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      if (opener && document.contains(opener)) opener.focus();
    };
  }, []);

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 backdrop-blur-sm animate-fade-in-up"
      role="dialog"
      aria-modal="true"
      aria-labelledby="confirm-title"
    >
      <div
        ref={dialogRef}
        className="bg-zinc-900 border border-zinc-800 rounded-2xl p-6 max-w-sm w-full shadow-2xl"
      >
        <div className="flex items-center gap-3 mb-4">
          <div className="w-10 h-10 rounded-full bg-red-500/10 flex items-center justify-center text-red-500">
            <IconTrash />
          </div>
          <h3 id="confirm-title" className="text-lg font-bold text-zinc-100">
            Delete permanently?
          </h3>
        </div>

        <p className="text-sm font-mono text-zinc-300 bg-zinc-950 p-2 rounded-lg break-all border border-zinc-800 mb-3">
          {dir ? joinPath(dir, target.name) : target.name}
        </p>

        {unscanned ? (
          <p className="text-sm text-zinc-400 mb-4">
            <span className="font-semibold text-zinc-200">
              Contents were not scanned — its real size is unknown.
            </span>{" "}
            Everything inside will be deleted. It cannot be undone.
          </p>
        ) : (
          <p className="text-sm text-zinc-400 mb-4">
            This frees{" "}
            <span className="font-semibold text-zinc-200">{formatBytes(target.size)}</span>
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
        )}

        {heavy && (
          <label className="block mb-5">
            <span className="text-xs text-zinc-500">
              {untypeable && "This name cannot be typed exactly. "}Type{" "}
              <span className="font-mono text-zinc-300">{phrase}</span> to confirm
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
