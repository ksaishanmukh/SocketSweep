import { useCallback, type MutableRefObject } from "react";
import { commands, errorCode, type Row, type SizeMetric, type Totals, type View } from "../api";
import { formatNumber } from "../lib/format";
import { isWithin, joinPath } from "../lib/path";
import { sameNode } from "../lib/size";
import type { Notify, SessionDeps } from "./types";
import type { useAnalyses } from "./useAnalyses";
import type { useNavigation } from "./useNavigation";
import type { useSearch } from "./useSearch";

interface Deps extends SessionDeps {
  notify: Notify;
  nav: ReturnType<typeof useNavigation>;
  analyses: ReturnType<typeof useAnalyses>;
  search: ReturnType<typeof useSearch>;
  setTotals: (totals: Totals) => void;
}

/**
 * Deletes a file or folder from the phone, by `NodeRef`.
 *
 * The host answers with the tree's new totals and the focused folder
 * refreshed, which is shown as is: if the open folder was inside what was
 * deleted, that is its nearest surviving ancestor.
 */
export function useDelete({
  sessionRef,
  metricRef,
  log,
  fail,
  notify,
  nav,
  analyses,
  search,
  setTotals,
}: Deps) {
  /**
   * `path` is the full path the confirmation showed; here it only decides
   * which search hits lived inside a deleted folder.
   */
  return useCallback(
    async (row: Row, path?: string) => {
      const session = sessionRef.current;
      if (session === null || row.node.session !== session) {
        notify("Results are out of date — rescan.", "error");
        return;
      }
      log(`[DELETE] ${row.name}…`);
      try {
        const result = await commands.delete(row.node);
        if (session !== sessionRef.current) return;
        setTotals(result.totals);
        const before = nav.currentView();
        nav.showView(result.view);
        // The host caps the returned view at one page. If the same folder had
        // more rows loaded, fetch them again so the list does not collapse.
        const wanted = before && sameNode(before.node, result.view.node) ? before.rows.length : 0;
        await refetchLoadedRows({ view: result.view, wanted, session, sessionRef, metricRef, nav });
        // A deleted node may still be sitting in the results list, along with
        // anything that lived inside it.
        const deletedPath = path ?? joinPath(row.parent ?? before?.path ?? "", row.name);
        search.dropHits(isInsideDeleted(row, deletedPath));
        search.rerun();
        void analyses.refresh();
        notify(`Deleted ${row.name}`, "success");
        log(`[DELETE] Removed ${formatNumber(result.items)} items.`);
      } catch (err) {
        fail(err);
        // "Stale" and "removed" mean the screen is out of step with the host;
        // the contract says the GUI refreshes.
        const code = errorCode(err);
        if (code === "stale" || code === "removed") {
          void nav.reload();
          search.rerun();
          void analyses.refresh();
        }
      }
    },
    [analyses, fail, log, metricRef, nav, notify, search, sessionRef, setTotals],
  );
}

async function refetchLoadedRows({
  view,
  wanted,
  session,
  sessionRef,
  metricRef,
  nav,
}: {
  view: View;
  wanted: number;
  session: number;
  sessionRef: MutableRefObject<number | null>;
  metricRef: MutableRefObject<SizeMetric>;
  nav: ReturnType<typeof useNavigation>;
}) {
  if (wanted <= view.rows.length || view.hidden <= 0) return;
  try {
    // The live views follow the last `focus`, not `getView`.
    void commands.focus(view.node, wanted, metricRef.current).catch(() => {});
    const full = await commands.getView(view.node, wanted, metricRef.current);
    if (session === sessionRef.current) nav.applyLiveView(full);
  } catch {
    // The capped view is already showing; "Load more" still works.
  }
}

function isInsideDeleted(row: Row, deletedPath: string) {
  const folder = row.kind === "dir" ? deletedPath : null;
  return (hit: Row) =>
    sameNode(hit.node, row.node) ||
    (folder !== null && hit.parent !== null && isWithin(hit.parent, folder));
}
