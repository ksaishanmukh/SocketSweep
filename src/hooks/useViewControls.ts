import { useCallback, useEffect } from "react";
import type { Category, NodeRef, Row, SizeMetric, SortKey } from "../api";
import { storePrefs } from "../lib/prefs";
import { NO_FILTERS, nextSort, toggleOption, type FilterOption } from "../lib/viewOptions";
import type { SessionDeps } from "./types";
import type { useDisplayChoice, Mode } from "./useDisplayChoice";
import type { TreeParts } from "./useScanSession";
import type { ViewOptionsState } from "./useViewOptions";

/** Typing pauses this long before the open folder is narrowed, so each keystroke is not a query. */
const QUERY_DEBOUNCE_MS = 150;

interface ControlDeps {
  options: ViewOptionsState;
  parts: TreeParts;
  storeMetric: (next: SizeMetric) => void;
  deps: SessionDeps;
}

/**
 * The list's controls. The host does the sorting and filtering, over every
 * child of the folder rather than only the loaded rows, so each change asks
 * again: the open folder, its map, and the everywhere search if one is up.
 */
export function useViewControls({ options, parts, storeMetric, deps }: ControlDeps) {
  const { nav, overview, search } = parts;
  const { reload } = nav;
  const { rerun } = search;
  const { setSort: storeSort, setFilters: storeFilters, sort, filters } = options;

  const requery = useCallback(() => {
    void reload();
    rerun();
  }, [reload, rerun]);

  const setSortKey = useCallback(
    (key: SortKey) => {
      const next = nextSort(sort, key);
      storeSort(next);
      storePrefs({ sort: next });
      requery();
    },
    [requery, sort, storeSort],
  );

  const toggleFilter = useCallback(
    (option: FilterOption) => {
      const next = toggleOption(filters, option);
      storeFilters(next);
      storePrefs({ filters: next });
      requery();
    },
    [filters, requery, storeFilters],
  );

  const clearFilters = useCallback(() => {
    storeFilters(NO_FILTERS);
    storePrefs({ filters: NO_FILTERS });
    requery();
  }, [requery, storeFilters]);

  const clearSearchAndFilters = useCallback(() => {
    options.setText("");
    options.applyQuery("");
    clearFilters();
  }, [clearFilters, options]);

  // Changing the size re-sorts everything the host sorted for us, so ask again.
  const { metricRef } = deps;
  const { refresh: refreshOverview } = overview;
  const setMetric = useCallback(
    (next: SizeMetric) => {
      if (metricRef.current === next) return;
      storeMetric(next);
      requery();
      void refreshOverview();
    },
    [metricRef, refreshOverview, requery, storeMetric],
  );

  useFolderQuery(options, reload);

  return { setSortKey, toggleFilter, clearFilters, clearSearchAndFilters, setMetric };
}

/** Narrows the open folder to what the box says, once typing pauses; searching everywhere leaves it whole. */
function useFolderQuery(options: ViewOptionsState, reload: () => Promise<void>) {
  const { text, everywhere, applyQuery } = options;
  useEffect(() => {
    const wanted = everywhere ? "" : text;
    const timer = setTimeout(() => {
      if (applyQuery(wanted)) void reload();
    }, QUERY_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [applyQuery, everywhere, reload, text]);
}

interface ActionDeps {
  display: ReturnType<typeof useDisplayChoice>;
  options: ViewOptionsState;
  parts: TreeParts;
  rootNode: () => NodeRef | null;
}

/** The jumps between and within the tabs. */
export function useBrowseActions({ display, options, parts, rootNode }: ActionDeps) {
  const { nav, overview } = parts;
  const { open, refreshTreemap } = nav;
  const { setMode: storeMode } = display;
  const { clearText, setFilters } = options;

  /** A tab shows a snapshot that may be old by now, so it is re-read on the way in. */
  const setMode = useCallback(
    (next: Mode) => {
      storeMode(next);
      if (next === "overview") void overview.refresh();
      else refreshTreemap();
    },
    [overview, refreshTreemap, storeMode],
  );

  /** A search belongs to the folder it was typed in, so going elsewhere empties the box. */
  const openFolder = useCallback(
    (node: NodeRef) => {
      clearText();
      return open(node);
    },
    [clearText, open],
  );

  const explore = useCallback(
    (node: NodeRef) => {
      storeMode("explorer");
      return openFolder(node);
    },
    [openFolder, storeMode],
  );

  /** Finding a huge file is only half the job; this shows the folder it lives in. */
  const reveal = useCallback(
    async (row: Row) => {
      if (row.parentNode) await explore(row.parentNode);
    },
    [explore],
  );

  /** One category of the whole phone: the root, narrowed to that type and nothing else. */
  const showCategory = useCallback(
    async (category: Category) => {
      const root = rootNode();
      if (!root) return;
      setFilters({ ...NO_FILTERS, categories: [category] });
      await explore(root);
    },
    [explore, rootNode, setFilters],
  );

  return { setMode, openFolder, explore, reveal, showCategory };
}
