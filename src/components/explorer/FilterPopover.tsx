import { useRef, type ReactNode, type RefObject } from "react";
import { formatNumber } from "../../lib/format";
import { CATEGORIES, CHANGED_BUCKETS, KINDS, categoryColor } from "../../lib/categories";
import { useDismiss } from "./useDismiss";
import { hasOption, optionLabel, type FilterOption, type Filters } from "../../lib/viewOptions";

const GROUPS: { title: string; options: FilterOption[] }[] = [
  { title: "Type", options: CATEGORIES.map((value) => ({ group: "categories", value })) },
  { title: "Show", options: KINDS.map((value) => ({ group: "kinds", value })) },
  { title: "Last changed", options: CHANGED_BUCKETS.map((value) => ({ group: "changed", value })) },
];

/**
 * The filter panel. Within a group the options are alternatives, and the
 * groups narrow together, which is how the host applies them. Closes on
 * Escape, on a click outside it and on Done, handing focus back to the
 * button that opened it.
 */
export function FilterPopover({
  filters,
  matching,
  total,
  anchorRef,
  onToggle,
  onReset,
  onClose,
  onDismiss,
}: {
  filters: Filters;
  matching: number;
  total: number;
  anchorRef: RefObject<HTMLElement | null>;
  onToggle: (option: FilterOption) => void;
  onReset: () => void;
  /** Escape and Done: focus goes back to the Filters button. */
  onClose: () => void;
  /** A press elsewhere: focus goes wherever the press put it. */
  onDismiss: () => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  useDismiss(panelRef, anchorRef, onClose, onDismiss);

  return (
    <div
      ref={panelRef}
      role="dialog"
      aria-label="Filters"
      className="absolute left-0 top-full mt-1 z-30 w-[34rem] max-w-[calc(100vw-2rem)] card shadow-xl p-4"
    >
      <div className="grid grid-cols-3 gap-4">
        {GROUPS.map((g) => (
          <Group key={g.title} title={g.title}>
            {g.options.map((o) => (
              <Option
                key={o.value}
                option={o}
                checked={hasOption(filters, o)}
                onToggle={onToggle}
              />
            ))}
          </Group>
        ))}
      </div>
      <Footer matching={matching} total={total} onReset={onReset} onDone={onClose} />
    </div>
  );
}

function Footer({
  matching,
  total,
  onReset,
  onDone,
}: {
  matching: number;
  total: number;
  onReset: () => void;
  onDone: () => void;
}) {
  return (
    <div className="flex items-center gap-2 mt-4 pt-3 border-t border-line">
      <span className="text-[13px] text-muted flex-1 num" aria-live="polite">
        {formatNumber(matching)} of {formatNumber(total)} items match
      </span>
      <button
        type="button"
        onClick={onReset}
        className="min-h-9 px-3 rounded-lg text-[13px] hover:bg-sunk"
      >
        Reset
      </button>
      <button
        type="button"
        onClick={onDone}
        className="min-h-9 px-4 rounded-lg text-[13px] font-medium bg-accent text-on-accent"
      >
        Done
      </button>
    </div>
  );
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <fieldset className="min-w-0">
      <legend className="text-[12px] font-semibold text-muted mb-1">{title}</legend>
      {children}
    </fieldset>
  );
}

function Option({
  option,
  checked,
  onToggle,
}: {
  option: FilterOption;
  checked: boolean;
  onToggle: (option: FilterOption) => void;
}) {
  return (
    <label className="flex items-center gap-2 min-h-9 text-[13px] cursor-pointer">
      <input type="checkbox" checked={checked} onChange={() => onToggle(option)} />
      {option.group === "categories" && (
        <span
          className="w-2.5 h-2.5 rounded-sm shrink-0"
          style={{ background: categoryColor(option.value) }}
          aria-hidden
        />
      )}
      <span className="truncate">{optionLabel(option)}</span>
    </label>
  );
}
