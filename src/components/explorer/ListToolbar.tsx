import type { Crumb, NodeRef } from "../../api";
import type { ScanSession } from "../../hooks/useScanSession";
import { nodeKey } from "../../lib/size";
import { IconBack, IconForward, IconSearch } from "../icons";

/** The root's crumb is named for what it is to a person, not by its path. */
const ROOT_LABEL = "Phone storage";

/** Back, forward, where the open folder is, and the search box, on one row. */
export function ListToolbar({ session }: { session: ScanSession }) {
  const { nav, actions, options } = session;
  const crumbs = nav.crumbs;
  const here = crumbs.length > 1 ? crumbs[crumbs.length - 1].name : ROOT_LABEL;
  return (
    <div className="flex items-center gap-2 px-3 py-2 border-b border-line">
      <button
        type="button"
        onClick={() => void nav.back()}
        disabled={!nav.canBack}
        aria-label="Back"
        title="Back (Alt+Left)"
        className="w-9 h-9 rounded-lg inline-flex items-center justify-center hover:bg-sunk disabled:opacity-40"
      >
        <IconBack />
      </button>
      <button
        type="button"
        onClick={() => void nav.forward()}
        disabled={!nav.canForward}
        aria-label="Forward"
        title="Forward (Alt+Right)"
        className="w-9 h-9 rounded-lg inline-flex items-center justify-center hover:bg-sunk disabled:opacity-40"
      >
        <IconForward />
      </button>
      <Breadcrumbs crumbs={crumbs} onOpen={(n) => void actions.openFolder(n)} />
      <div className="flex-1" />
      <SearchBox
        text={options.text}
        everywhere={options.everywhere}
        placeholder={options.everywhere ? "Search everywhere" : `Search in ${here}`}
        onText={options.setText}
        onEverywhere={options.setEverywhere}
      />
    </div>
  );
}

function Breadcrumbs({ crumbs, onOpen }: { crumbs: Crumb[]; onOpen: (node: NodeRef) => void }) {
  return (
    <nav aria-label="Breadcrumb" className="min-w-0 flex-[0_1_auto] overflow-hidden">
      <ol className="flex items-center text-[14px] min-w-0">
        {crumbs.map((crumb, idx) => {
          const last = idx === crumbs.length - 1;
          return (
            <li key={nodeKey(crumb.node)} className="inline-flex items-center min-w-0">
              {idx > 0 && (
                <span className="mx-1 text-faint shrink-0" aria-hidden>
                  /
                </span>
              )}
              <button
                type="button"
                onClick={() => onOpen(crumb.node)}
                aria-current={last ? "page" : undefined}
                title={idx === 0 ? ROOT_LABEL : crumb.name}
                className={`truncate min-h-9 px-1 rounded hover:underline ${
                  last ? "font-semibold text-fg" : "text-muted"
                }`}
              >
                {idx === 0 ? ROOT_LABEL : crumb.name}
              </button>
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

function SearchBox({
  text,
  everywhere,
  placeholder,
  onText,
  onEverywhere,
}: {
  text: string;
  everywhere: boolean;
  placeholder: string;
  onText: (text: string) => void;
  onEverywhere: (on: boolean) => void;
}) {
  return (
    <div className="flex items-center gap-2 shrink-0">
      <div className="relative">
        <span className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted">
          <IconSearch />
        </span>
        <input
          type="search"
          value={text}
          onChange={(e) => onText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") onText("");
          }}
          placeholder={placeholder}
          aria-label={placeholder}
          className="w-64 h-9 rounded-lg bg-sunk border border-line pl-8 pr-2 text-[13px] placeholder:text-faint"
        />
      </div>
      <label className="inline-flex items-center gap-1.5 text-[12px] text-muted min-h-9 cursor-pointer">
        <input
          type="checkbox"
          checked={everywhere}
          onChange={(e) => onEverywhere(e.target.checked)}
        />
        Everywhere
      </label>
    </div>
  );
}
