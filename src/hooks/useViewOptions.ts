import { useCallback, useMemo, useRef, useState } from "react";
import type { ViewSort } from "../api";
import { DEFAULT_SORT, NO_FILTERS, filtersOf, viewFilter, type Filters } from "../lib/viewOptions";
import type { ViewOptions } from "./types";

const DEFAULT_OPTIONS: ViewOptions = { sort: DEFAULT_SORT, filter: viewFilter(NO_FILTERS, "") };

/**
 * How the list is asked for: sort, filters and the search box.
 *
 * The box searches the open folder by narrowing its view (the host filters
 * every child, not just the loaded ones), or, with `everywhere`, the whole
 * phone with `search`. `optionsRef` holds what the next view query sends;
 * it changes synchronously so a reload right after a change already sees it.
 * Reloading is the caller's job, since the folder and search hooks own the
 * queries.
 */
export function useViewOptions() {
  const optionsRef = useRef<ViewOptions>(DEFAULT_OPTIONS);
  const [options, setOptions] = useState<ViewOptions>(DEFAULT_OPTIONS);
  const [text, setText] = useState("");
  const [everywhere, setEverywhere] = useState(false);
  const filters = useMemo(() => filtersOf(options.filter), [options.filter]);

  const commit = useCallback((next: ViewOptions) => {
    optionsRef.current = next;
    setOptions(next);
  }, []);

  const setSort = useCallback(
    (sort: ViewSort) => commit({ ...optionsRef.current, sort }),
    [commit],
  );

  const setFilters = useCallback(
    (filters: Filters) => {
      const query = optionsRef.current.filter.query ?? "";
      commit({ ...optionsRef.current, filter: viewFilter(filters, query) });
    },
    [commit],
  );

  /** Puts `query` into the view filter; false when it was there already. */
  const applyQuery = useCallback(
    (query: string) => {
      const current = optionsRef.current.filter;
      const next = viewFilter(filtersOf(current), query);
      if (next.query === current.query) return false;
      commit({ ...optionsRef.current, filter: next });
      return true;
    },
    [commit],
  );

  /** Empties the box at once, e.g. on opening another folder. */
  const clearText = useCallback(() => {
    setText("");
    applyQuery("");
  }, [applyQuery]);

  return {
    optionsRef,
    sort: options.sort,
    filters,
    /** The name search the open folder's view is narrowed by, as applied. */
    folderQuery: options.filter.query,
    text,
    everywhere,
    setSort,
    setFilters,
    setText,
    setEverywhere,
    applyQuery,
    clearText,
  };
}

export type ViewOptionsState = ReturnType<typeof useViewOptions>;
