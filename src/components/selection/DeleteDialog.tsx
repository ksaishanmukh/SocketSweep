import { useRef, useState } from "react";
import { useFocusTrap } from "../../hooks/useFocusTrap";
import { formatBytes, plural } from "../../lib/format";
import { needsTypedConfirm, totalOf, type Target } from "../../lib/selection";
import { IconTrash } from "../icons";

/** What a big or uncertain batch is confirmed with. Fixed, since names can be lossy or untypeable. */
export const CONFIRM_WORD = "DELETE";

/**
 * Deleting is the one irreversible thing this app does.
 *
 * The dialog lists every path that goes, with its size, and leads with what
 * will be freed, because that is what prevents mistakes. Sizes are disk
 * usage, whatever the main view shows: that is what deleting frees. A batch
 * that is big, or holds folders not fully measured, must be confirmed by
 * typing. Focus is trapped while open and starts on Cancel.
 */
export function DeleteDialog({
  targets,
  onCancel,
  onConfirm,
}: {
  targets: Target[];
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const [typed, setTyped] = useState("");
  const cancelRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  useFocusTrap(dialogRef, cancelRef, onCancel);

  const typing = needsTypedConfirm(targets);
  const armed = !typing || typed.trim() === CONFIRM_WORD;

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 p-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="delete-title"
      aria-describedby="delete-consequence"
    >
      <div ref={dialogRef} className="card shadow-2xl p-5 w-full max-w-lg rise-in">
        <div className="flex items-center gap-3 mb-3">
          <span className="w-9 h-9 rounded-full bg-sunk flex items-center justify-center text-danger">
            <IconTrash />
          </span>
          <h2 id="delete-title" className="text-[17px] font-semibold">
            Delete {plural(targets.length, "item")}?
          </h2>
        </div>

        <TargetList targets={targets} />

        <Consequence targets={targets} />

        {typing && <TypeToConfirm typed={typed} onChange={setTyped} />}

        <div className="flex items-center justify-end gap-2">
          <button
            ref={cancelRef}
            type="button"
            onClick={onCancel}
            className="min-h-10 px-4 rounded-lg text-[14px] font-medium hover:bg-sunk"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={!armed}
            className="min-h-10 px-4 rounded-lg text-[14px] font-semibold bg-danger text-on-danger disabled:opacity-40"
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  );
}

function TargetList({ targets }: { targets: Target[] }) {
  return (
    <ul className="max-h-56 overflow-y-auto rounded-lg bg-sunk border border-line p-2 mb-3">
      {targets.map((t) => (
        <li key={t.key} className="flex items-baseline gap-3 py-0.5">
          <span className="font-mono text-[12px] break-all flex-1">
            {t.path ?? `Everything ${t.name} left behind (${plural(t.nodes.length, "folder")})`}
          </span>
          <span className="num text-[12px] text-muted shrink-0">
            {t.measure === "unknown" ? "not measured" : formatBytes(t.size.disk)}
            {t.measure === "partial" ? "+" : ""}
          </span>
        </li>
      ))}
    </ul>
  );
}

function TypeToConfirm({ typed, onChange }: { typed: string; onChange: (typed: string) => void }) {
  return (
    <label className="block mb-4">
      <span className="text-[13px] text-muted">
        Type <span className="font-mono font-semibold text-fg">{CONFIRM_WORD}</span> to confirm
      </span>
      <input
        value={typed}
        onChange={(e) => onChange(e.target.value)}
        autoComplete="off"
        spellCheck={false}
        className="mt-1 w-full h-10 rounded-lg bg-sunk border border-line px-3 font-mono text-[14px]"
      />
    </label>
  );
}

/** What will be freed. A folder still being measured may hold more than it shows, and says so. */
function Consequence({ targets }: { targets: Target[] }) {
  const total = totalOf(targets, "disk");
  return (
    <p id="delete-consequence" className="text-[14px] text-muted mb-4">
      This frees {total.uncertain && "at least "}
      <span className="num font-semibold text-fg">{formatBytes(total.bytes)}</span>
      {total.uncertain && " (still counting)"}
      {total.files > 0 && <> across {plural(total.files, "file")}</>}. It can’t be undone.
    </p>
  );
}
