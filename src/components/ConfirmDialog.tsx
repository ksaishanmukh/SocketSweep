import { useRef, useState, type RefObject } from "react";
import { formatBytes, formatNumber } from "../lib/format";
import type { Row } from "../api";
import { useFocusTrap } from "../hooks/useFocusTrap";
import { IconTrash } from "./icons";

/** Above this, the dialog asks the user to type the name rather than just click. */
const TYPE_TO_CONFIRM_BYTES = 1024 ** 3;
const TYPE_TO_CONFIRM_FILES = 100;

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
  useFocusTrap(dialogRef, cancelRef, onCancel);

  const rules = confirmationRules(target, typed);

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

        <Consequence target={target} rules={rules} />

        {rules.heavy && <TypeToConfirm rules={rules} typed={typed} onChange={setTyped} />}

        <DialogActions
          cancelRef={cancelRef}
          armed={rules.armed}
          onCancel={onCancel}
          onConfirm={onConfirm}
        />
      </div>
    </div>
  );
}

function DialogActions({
  cancelRef,
  armed,
  onCancel,
  onConfirm,
}: {
  cancelRef: RefObject<HTMLButtonElement | null>;
  armed: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
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
  );
}

interface ConfirmationRules {
  isDir: boolean;
  freed: number;
  /** Not measured yet: the size says nothing about what is there. */
  unscanned: boolean;
  /** Still being counted: the number is a lower bound. */
  counting: boolean;
  /** Lossy names cannot be retyped, so they fall back to a fixed word. */
  untypeable: boolean;
  phrase: string;
  /** Large enough, or unknown enough, that clicking is not confirmation. */
  heavy: boolean;
  /** The confirm button may be pressed. */
  armed: boolean;
}

function confirmationRules(target: Row, typed: string): ConfirmationRules {
  const isDir = target.kind === "dir";
  const freed = target.size.disk;
  const unscanned = target.measure === "unknown";
  const counting = target.measure === "partial";
  const untypeable = target.name.includes("�") || target.name.trim() === "";
  const phrase = untypeable ? "DELETE" : target.name;
  // A folder that is not fully measured may hold far more than it shows.
  const heavy =
    (isDir && (unscanned || counting)) ||
    freed >= TYPE_TO_CONFIRM_BYTES ||
    target.files >= TYPE_TO_CONFIRM_FILES;
  const armed = !heavy || typed.normalize("NFC") === phrase.normalize("NFC");
  return { isDir, freed, unscanned, counting, untypeable, phrase, heavy, armed };
}

function Consequence({ target, rules }: { target: Row; rules: ConfirmationRules }) {
  const { isDir, freed, unscanned, counting } = rules;
  if (unscanned) {
    return (
      <p className="text-sm text-zinc-400 mb-4">
        <span className="font-semibold text-zinc-200">
          Not measured yet — its real size is unknown.
        </span>{" "}
        {isDir ? "Everything inside will be deleted. " : ""}It cannot be undone.
      </p>
    );
  }
  return (
    <p className="text-sm text-zinc-400 mb-4">
      This frees {counting && "at least "}
      <span className="font-semibold text-zinc-200">{formatBytes(freed)}</span>
      {counting && " (still counting)"}
      {isDir && (
        <>
          {" "}
          across{" "}
          <span className="font-semibold text-zinc-200">{formatNumber(target.files)} files</span>
        </>
      )}
      . It cannot be undone.
    </p>
  );
}

function TypeToConfirm({
  rules,
  typed,
  onChange,
}: {
  rules: ConfirmationRules;
  typed: string;
  onChange: (typed: string) => void;
}) {
  return (
    <label className="block mb-5">
      <span className="text-xs text-zinc-500">
        {rules.untypeable && "This name cannot be typed exactly. "}Type{" "}
        <span className="font-mono text-zinc-300">{rules.phrase}</span> to confirm
      </span>
      <input
        value={typed}
        onChange={(e) => onChange(e.target.value)}
        autoComplete="off"
        spellCheck={false}
        className="mt-1 w-full bg-zinc-950 border border-zinc-800 rounded-lg px-2 py-1.5 text-sm font-mono text-zinc-200
                focus-visible:outline-2 focus-visible:outline-accent-500"
      />
    </label>
  );
}
