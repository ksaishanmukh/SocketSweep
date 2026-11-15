import { useRef, useState } from "react";
import type { ScanSession } from "../../hooks/useScanSession";
import { formatBytes, formatNumber, plural } from "../../lib/format";
import { sizeOf } from "../../lib/size";
import { activeOptions, filterCount, optionLabel } from "../../lib/viewOptions";
import { IconClose, IconFilter } from "../icons";
import { FilterPopover } from "./FilterPopover";

/** The Filters button and its panel, the active filters as chips, and how much is shown. */
export function FilterBar({ session, searching }: { session: ScanSession; searching: boolean }) {
  const { options, controls } = session;
  const [open, setOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const view = session.nav.view!;
  const count = filterCount(options.filters);

  const close = () => {
    setOpen(false);
    buttonRef.current?.focus();
  };

  return (
    <div className="flex flex-wrap items-center gap-2 px-3 py-2 border-b border-line">
      <div className="relative">
        <button
          ref={buttonRef}
          type="button"
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
          aria-haspopup="dialog"
          className={`min-h-9 px-3 rounded-lg border text-[13px] font-medium inline-flex items-center gap-2 ${
            count > 0 ? "border-accent text-accent" : "border-line hover:bg-sunk"
          }`}
        >
          <IconFilter />
          Filters
          {count > 0 && (
            <span className="num min-w-5 h-5 px-1 rounded-full bg-accent text-on-accent text-[11px] inline-flex items-center justify-center">
              {count}
            </span>
          )}
        </button>
        {open && (
          <FilterPopover
            filters={options.filters}
            matching={view.totalMatching}
            total={view.totalChildren}
            anchorRef={buttonRef}
            onToggle={controls.toggleFilter}
            onReset={controls.clearFilters}
            onClose={close}
            onDismiss={() => setOpen(false)}
          />
        )}
      </div>
      <Chips session={session} />
      <div className="flex-1" />
      <Shown session={session} searching={searching} />
    </div>
  );
}

function Chips({ session }: { session: ScanSession }) {
  const { options, controls } = session;
  const active = activeOptions(options.filters);
  if (active.length === 0) return null;
  return (
    <>
      <ul className="flex flex-wrap items-center gap-1.5" aria-label="Active filters">
        {active.map((o) => (
          <li key={`${o.group}:${o.value}`}>
            <button
              type="button"
              onClick={() => controls.toggleFilter(o)}
              aria-label={`Remove filter ${optionLabel(o)}`}
              className="min-h-8 pl-2.5 pr-2 rounded-full bg-sunk border border-line text-[12px] inline-flex items-center gap-1.5 hover:border-muted"
            >
              {optionLabel(o)}
              <IconClose />
            </button>
          </li>
        ))}
      </ul>
      <button
        type="button"
        onClick={controls.clearFilters}
        className="min-h-8 px-1 text-[12px] text-accent hover:underline"
      >
        Clear all
      </button>
    </>
  );
}

/** "7 of 20 shown · 36.5 GB · 1,263 files", or the result count of an everywhere search. */
function Shown({ session, searching }: { session: ScanSession; searching: boolean }) {
  const { nav, metric, searchResults, options } = session;
  const view = nav.view!;
  if (searching) {
    return (
      <span className="text-[13px] text-muted num">
        {searchResults === null ? "Searching…" : plural(searchResults.length, "result")}
      </span>
    );
  }
  const narrowed = filterCount(options.filters) > 0 || options.folderQuery !== null;
  const bytes = narrowed
    ? view.rows.reduce((sum, r) => sum + sizeOf(r.size, metric), 0)
    : sizeOf(view.size, metric);
  const files = narrowed ? view.rows.reduce((sum, r) => sum + r.files, 0) : view.files;
  const atLeast = narrowed && view.hidden > 0 ? "at least " : "";
  return (
    <span className="text-[13px] text-muted num">
      {formatNumber(view.totalMatching)} of {formatNumber(view.totalChildren)} shown · {atLeast}
      {formatBytes(bytes)} · {plural(files, "file")}
    </span>
  );
}
