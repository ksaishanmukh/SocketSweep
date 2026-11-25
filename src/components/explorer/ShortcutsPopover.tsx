import { useRef, type RefObject } from "react";
import { useDismiss } from "./useDismiss";

const GROUPS: { title: string; keys: [string, string][] }[] = [
  {
    title: "In the list",
    keys: [
      ["Up / Down", "Move to the previous or next row"],
      ["Home / End", "First or last row"],
      ["Page Up / Page Down", "Move a page"],
      ["Enter", "Open the folder"],
      ["Backspace or Alt+Left", "Back"],
      ["Alt+Right", "Forward"],
      ["Space", "Select or unselect the row"],
      ["Shift+Up / Down", "Extend the selection"],
      ["Ctrl+A", "Select every row shown"],
      ["Escape", "Clear the selection"],
      ["Delete", "Mark the selection (or the row) for deletion"],
    ],
  },
  {
    title: "Anywhere in the Explorer",
    keys: [
      ["Ctrl+F", "Search"],
      ["?", "Show this list"],
    ],
  },
];

/** What the keys do. Closes like the filter panel: Escape, a press outside, or Close. */
export function ShortcutsPopover({
  anchorRef,
  onClose,
  onDismiss,
}: {
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
  onDismiss: () => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  useDismiss(panelRef, anchorRef, onClose, onDismiss);
  return (
    <div
      ref={panelRef}
      role="dialog"
      aria-label="Keyboard shortcuts"
      className="absolute right-0 top-full mt-1 z-30 w-[26rem] max-w-[calc(100vw-2rem)] card shadow-xl p-4"
    >
      {GROUPS.map((g) => (
        <section key={g.title} aria-label={g.title} className="mb-3">
          <h3 className="text-[12px] font-semibold text-muted mb-1">{g.title}</h3>
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-[13px]">
            {g.keys.map(([key, what]) => (
              <div key={key} className="contents">
                <dt className="font-mono text-[12px]">{key}</dt>
                <dd>{what}</dd>
              </div>
            ))}
          </dl>
        </section>
      ))}
      <div className="flex justify-end">
        <button
          type="button"
          onClick={onClose}
          className="min-h-9 px-4 rounded-lg text-[13px] font-medium bg-accent text-on-accent"
        >
          Close
        </button>
      </div>
    </div>
  );
}
