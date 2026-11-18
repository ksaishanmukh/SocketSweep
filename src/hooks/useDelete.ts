import { useCallback, type MutableRefObject } from "react";
import { commands, errorCode, type DeletedMany, type Row, type Totals, type View } from "../api";
import { formatBytes, formatNumber, plural } from "../lib/format";
import { isWithin } from "../lib/path";
import { inSession, type Target } from "../lib/selection";
import { nodeKey, sameNode } from "../lib/size";
import type { Notify, SessionDeps, ViewOptions } from "./types";
import type { useNavigation } from "./useNavigation";
import type { useOverview } from "./useOverview";
import type { useSearch } from "./useSearch";
import type { Selection } from "./useSelection";

interface Deps extends SessionDeps {
  notify: Notify;
  nav: ReturnType<typeof useNavigation>;
  overview: ReturnType<typeof useOverview>;
  search: ReturnType<typeof useSearch>;
  selection: Selection;
  setTotals: (totals: Totals) => void;
}

/** How many failed paths a toast names before it summarises the rest. */
const FAILURES_NAMED = 3;

/**
 * Deletes one or more files, folders or leftovers from the phone, by
 * `NodeRef`, with one `deleteMany`.
 *
 * The host answers with the tree's new totals and the focused folder
 * refreshed, which is shown as is: if the open folder was inside what was
 * deleted, that is its nearest surviving ancestor. A node the phone refused
 * stays on screen and picked, and is named in the toast.
 */
export function useDelete(deps: Deps) {
  const { sessionRef, metricRef, optionsRef, log, fail, notify } = deps;
  const { nav, overview, search, selection, setTotals } = deps;
  return useCallback(
    async (targets: Target[]) => {
      const session = sessionRef.current;
      const nodes = targets.flatMap((t) => t.nodes);
      if (nodes.length === 0) return;
      if (session === null || !targets.every((t) => inSession(t, session))) {
        notify("Results are out of date — rescan.", "error");
        return;
      }
      log(`[DELETE] ${plural(targets.length, "item")}…`);
      try {
        const result = await commands.deleteMany(nodes);
        if (session !== sessionRef.current) return;
        setTotals(result.totals);
        const before = nav.currentView();
        nav.showView(result.view);
        const wanted = before && sameNode(before.node, result.view.node) ? before.rows.length : 0;
        await refetchLoadedRows({
          sessionRef,
          metricRef,
          optionsRef,
          nav,
          view: result.view,
          wanted,
          session,
        });
        forgetDeleted(targets, result, { selection, search, overview });
        report(result, targets, nodes.length, notify);
        log(`[DELETE] Removed ${formatNumber(result.items)} items.`);
      } catch (err) {
        fail(err);
        // "Stale" and "removed" mean the screen is out of step with the host,
        // so reload it.
        const code = errorCode(err);
        if (code === "stale" || code === "removed") {
          void nav.reload();
          search.rerun();
          void overview.refresh();
        }
      }
    },
    [
      fail,
      log,
      metricRef,
      nav,
      notify,
      optionsRef,
      overview,
      search,
      selection,
      sessionRef,
      setTotals,
    ],
  );
}

async function refetchLoadedRows({
  view,
  wanted,
  session,
  sessionRef,
  metricRef,
  optionsRef,
  nav,
}: {
  view: View;
  wanted: number;
  session: number;
  sessionRef: MutableRefObject<number | null>;
  metricRef: SessionDeps["metricRef"];
  optionsRef: MutableRefObject<ViewOptions>;
  nav: ReturnType<typeof useNavigation>;
}) {
  // The host caps the returned view at one page. If the same folder had more
  // rows loaded, fetch them again so the list does not collapse.
  if (wanted <= view.rows.length || view.hidden <= 0) return;
  const { sort, filter } = optionsRef.current;
  try {
    // The live views follow the last `focus`, not `getView`.
    const request = { node: view.node, limit: wanted, by: metricRef.current, sort, filter };
    void commands.focus(request).catch(() => {});
    const full = await commands.getView(request);
    if (session === sessionRef.current) nav.applyLiveView(full);
  } catch {
    // The capped view is already showing; "Load more" still works.
  }
}

/** Everything that held a deleted node lets go of it; a refused node stays picked and listed. */
function forgetDeleted(
  targets: Target[],
  result: DeletedMany,
  { selection, search, overview }: Pick<Deps, "selection" | "search" | "overview">,
) {
  const failed = new Set(result.failures.map((f) => nodeKey(f.node)));
  const gone = targets.filter((t) => t.nodes.every((n) => !failed.has(nodeKey(n))));
  selection.forget(gone.flatMap((t) => t.nodes));
  // A deleted node may still be sitting in the search results, along with
  // anything that lived inside it.
  search.dropHits(isInsideDeleted(gone));
  search.rerun();
  void overview.refresh();
}

function isInsideDeleted(gone: Target[]) {
  const keys = new Set(gone.flatMap((t) => t.nodes.map(nodeKey)));
  const folders = gone.filter((t) => t.kind === "dir" && t.path !== null).map((t) => t.path!);
  return (hit: Row) =>
    keys.has(nodeKey(hit.node)) ||
    (hit.parent !== null && folders.some((folder) => isWithin(hit.parent!, folder)));
}

function report(result: DeletedMany, targets: Target[], asked: number, notify: Notify) {
  if (result.failures.length === 0) {
    const what = targets.length === 1 ? targets[0].name : plural(targets.length, "item");
    notify(`Deleted ${what}, freeing ${formatBytes(result.size.disk)}`, "success");
    return;
  }
  const named = result.failures
    .slice(0, FAILURES_NAMED)
    .map((f) => `${f.path} (${f.error.message})`)
    .join("; ");
  const more = result.failures.length - FAILURES_NAMED;
  notify(
    `Deleted ${formatNumber(result.deleted)} of ${plural(asked, "item")}. ` +
      `Could not delete ${named}${more > 0 ? ` and ${formatNumber(more)} more` : ""}.`,
    "error",
  );
}
