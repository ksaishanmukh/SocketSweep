import type { RefObject } from "react";
import type { Row, View } from "../../api";
import type { ListKeyActions } from "../../hooks/useListKeys";
import { VIEW_PAGE_SIZE, type ScanSession } from "../../hooks/useScanSession";
import { formatNumber } from "../../lib/format";
import { folderName } from "../../lib/path";
import { targetOfRow, type Target } from "../../lib/selection";
import { sizeOf } from "../../lib/size";
import { filterCount } from "../../lib/viewOptions";
import { FilterBar } from "./FilterBar";
import type { Hover, SetHover } from "./hover";
import { ListBody } from "./ListBody";
import { ListHeader } from "./ListHeader";
import type { RowActions } from "./ListRow";
import { ListToolbar, type Help } from "./ListToolbar";

/** The open folder's rows, or the results of a search everywhere, with everything that steers them. */
export function ListCard({
  session,
  view,
  hover,
  onHover,
  onDelete,
  searchRef,
  help,
}: {
  session: ScanSession;
  view: View;
  hover: Hover | null;
  onHover: SetHover;
  onDelete: (targets: Target[]) => void;
  searchRef: RefObject<HTMLInputElement | null>;
  help: Help;
}) {
  const { options, selection, controls, metric, searchResults } = session;
  const searching = options.everywhere && options.text.trim() !== "";
  const rows = searching ? (searchResults ?? []) : view.rows;
  const targetOf = (row: Row) => targetOfRow(row, view.path);
  const targets = rows.map(targetOf);
  const picked = targets.filter((t) => selection.has(t.key)).length;
  const folderBytes = sizeOf(view.size, metric);
  const actions = useRowActions(session, targetOf, onDelete);
  const keys = useListKeyActions(session, targetOf, onDelete);

  return (
    <section
      aria-label="Folder contents"
      className="card flex-1 min-h-[280px] flex flex-col overflow-hidden"
    >
      <ListToolbar session={session} searchRef={searchRef} help={help} />
      <FilterBar session={session} searching={searching} />
      <div
        role="grid"
        aria-label="Files and folders"
        aria-multiselectable
        aria-rowcount={rows.length + 1}
        className="flex-1 min-h-0 flex flex-col"
      >
        <ListHeader
          sort={options.sort}
          shown={rows.length}
          picked={picked}
          onSort={controls.setSortKey}
          onSelectAll={(on) => selection.setMany(targets, on)}
        />
        {rows.length === 0 ? (
          <Empty session={session} view={view} searching={searching} />
        ) : (
          <ListBody
            rows={rows}
            shareOf={(row) =>
              searching || folderBytes <= 0 ? null : sizeOf(row.size, metric) / folderBytes
            }
            isPicked={(row) => selection.has(targetOf(row).key)}
            hover={hover}
            onHover={onHover}
            actions={actions}
            keys={keys}
            resetKey={view.path}
            footer={!searching && view.hidden > 0 && <LoadMore session={session} view={view} />}
          />
        )}
      </div>
    </section>
  );
}

function useRowActions(
  session: ScanSession,
  targetOf: (row: Row) => Target,
  onDelete: (targets: Target[]) => void,
): RowActions {
  const { actions, selection, metric, copyDestination } = session;
  return {
    metric,
    onOpen: (row) => void actions.openFolder(row.node),
    onReveal: (row) => void actions.reveal(row),
    onToggle: (row) => selection.toggle(targetOf(row)),
    onCopy: (row) => void session.startCopy([targetOf(row)]),
    onDelete: (row) => onDelete([targetOf(row)]),
    copyTarget: copyDestination ? folderName(copyDestination) : null,
    copyBusy: session.copy !== null,
  };
}

function useListKeyActions(
  session: ScanSession,
  targetOf: (row: Row) => Target,
  onDelete: (targets: Target[]) => void,
): ListKeyActions {
  const { selection, nav } = session;
  return {
    back: () => void nav.back(),
    pick: (picked, on) => selection.setMany(picked.map(targetOf), on),
    clearPicks: selection.clear,
    // Same as the action bar's Delete when something is picked.
    deleteRows: (picked) =>
      onDelete(selection.items.length > 0 ? selection.items : picked.map(targetOf)),
  };
}

function Empty({
  session,
  view,
  searching,
}: {
  session: ScanSession;
  view: View;
  searching: boolean;
}) {
  const { options, controls, searchResults } = session;
  const narrowed = searching || filterCount(options.filters) > 0 || options.folderQuery !== null;
  if (searching && searchResults === null) {
    return <p className="p-8 text-center text-[14px] text-muted">Searching…</p>;
  }
  if (narrowed) {
    return (
      <div className="p-8 text-center text-[14px] text-muted">
        <p>Nothing here matches.</p>
        <button
          type="button"
          onClick={controls.clearSearchAndFilters}
          className="mt-2 min-h-9 px-3 rounded-lg text-accent hover:underline"
        >
          Clear search and filters
        </button>
      </div>
    );
  }
  return (
    <p className="p-8 text-center text-[14px] text-muted">
      {view.measure === "unknown" ? "Listing this folder…" : "This folder is empty."}
    </p>
  );
}

function LoadMore({ session, view }: { session: ScanSession; view: View }) {
  const loading = session.nav.loadingMore;
  return (
    <div className="flex items-center gap-3 px-3 pt-3">
      <span className="text-[12px] text-muted flex-1 num">
        Showing {formatNumber(view.rows.length)} of {formatNumber(view.rows.length + view.hidden)}
      </span>
      <button
        type="button"
        onClick={() => void session.nav.loadMore()}
        disabled={loading}
        className="min-h-9 px-3 rounded-lg border border-line text-[13px] hover:bg-sunk disabled:opacity-50"
      >
        {loading ? "Loading…" : `Load ${formatNumber(Math.min(VIEW_PAGE_SIZE, view.hidden))} more`}
      </button>
    </div>
  );
}
