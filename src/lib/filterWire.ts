import type { SearchFilter, ViewFilter } from "../api";
import type { Filters } from "./viewOptions";

/** What `getView` and `focus` take: the filters plus the in-folder name search. */
export function viewFilter(filters: Filters, query: string): ViewFilter {
  const trimmed = query.trim();
  return { query: trimmed === "" ? null : trimmed, ...filters };
}

/** What `search` takes: the same filters; the query travels separately. */
export function searchFilter({ categories, kinds, changed }: Filters): SearchFilter {
  return { categories, kinds, changed };
}

/** The filter groups of a view filter, without its name search. */
export function filtersOf({ categories, kinds, changed }: ViewFilter): Filters {
  return { categories, kinds, changed };
}
