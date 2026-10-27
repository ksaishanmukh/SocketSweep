import { useEffect, useRef, useState } from "react";
import { formatBytes, formatNumber } from "../lib/format";
import type { Row } from "../api";
import { IconTrash } from "./icons";

/** Above this, the dialog asks the user to type the name rather than just click. */
const TYPE_TO_CONFIRM_BYTES = 1024 ** 3;
const TYPE_TO_CONFIRM_FILES = 100;

const FOCUSABLE =
  'button:not([disabled]), input:not([disabled]), [href], [tabindex]:not([tabindex="-1"])';

/**
 * Deleting is the one irreversible thing this app does.
 *
 * The dialog leads with what will be lost — size, and file count for a folder —
 * because that is the information that prevents mistakes, and the host already
 * knows it. The size is always the *disk* usage: that is what deleting frees,
 * whichever size the main view is showing. It traps focus while open and hands
 * focus back to whatever opened it.
 */
export function ConfirmDialog({
  target,
  path,
  onCancel,
  onConfirm,
}: {
  target: Row;
  /** The target's full path on the phone, for display; the delete itself goes by `target.node`. */
  path: string;
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

  const isDir = target.kind === "dir";
  const freed = target.size.disk;
  // Not measured yet: the size says nothing about what is there.
  const unscanned = target.measure === "unknown";
  // Still being counted: the number is a lower bound.
  const counting = target.measure === "partial";
  // Lossy names cannot be retyped, so they fall back to a fixed word.
  const untypeable = target.name.includes("\uFFFD") || target.name.trim() === "";
  const phrase = untypeable ? "DELETE" : target.name;
  // A folder that is not fully measured may hold far more than it shows.
  const heavy =
    (isDir && (unscanned || counting)) ||
    freed >= TYPE_TO_CONFIRM_BYTES ||
    target.files >= TYPE_TO_CONFIRM_FILES;
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
          {path}
        </p>

        {unscanned ? (
          <p className="text-sm text-zinc-400 mb-4">
            <span className="font-semibold text-zinc-200">
              Not measured yet — its real size is unknown.
            </span>{" "}
            {isDir ? "Everything inside will be deleted. " : ""}It cannot be undone.
          </p>
        ) : (
          <p className="text-sm text-zinc-400 mb-4">
            This frees {counting && "at least "}
            <span className="font-semibold text-zinc-200">{formatBytes(freed)}</span>
            {counting && " (still counting)"}
            {isDir && (
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
