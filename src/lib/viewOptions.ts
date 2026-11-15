import type {
  Category,
  ChangedBucket,
  KindFilter,
  SearchFilter,
  SortKey,
  ViewFilter,
  ViewSort,
} from "../api";
import { CATEGORY_LABEL, CHANGED_LABEL, KIND_LABEL } from "./categories";

/** The filter panel's three groups: an option within a group is an alternative, groups must all match. */
export interface Filters {
  categories: Category[];
  kinds: KindFilter[];
  changed: ChangedBucket[];
}

export type FilterGroup = keyof Filters;

/** One option of one group, as the chips and the panel's checkboxes name it. */
export type FilterOption =
  | { group: "categories"; value: Category }
  | { group: "kinds"; value: KindFilter }
  | { group: "changed"; value: ChangedBucket };

export const NO_FILTERS: Filters = { categories: [], kinds: [], changed: [] };

export const DEFAULT_SORT: ViewSort = { key: "size", descending: true };

/**
 * The direction a column starts in when first clicked: names read A to Z,
 * everything else biggest or newest first, since that is what one looks for.
 */
const STARTS_DESCENDING: Record<SortKey, boolean> = {
  name: false,
  size: true,
  count: true,
  changed: true,
};

/** Clicking the active column flips it; clicking another starts it in its natural direction. */
export function nextSort(current: ViewSort, key: SortKey): ViewSort {
  if (current.key === key) return { key, descending: !current.descending };
  return { key, descending: STARTS_DESCENDING[key] };
}

export function hasOption(filters: Filters, option: FilterOption): boolean {
  return (filters[option.group] as string[]).includes(option.value);
}

export function toggleOption(filters: Filters, option: FilterOption): Filters {
  const list = filters[option.group] as string[];
  const next = list.includes(option.value)
    ? list.filter((v) => v !== option.value)
    : [...list, option.value];
  return { ...filters, [option.group]: next };
}

export function filterCount(filters: Filters): number {
  return filters.categories.length + filters.kinds.length + filters.changed.length;
}

/** The active options in panel order, for the removable chips. */
export function activeOptions(filters: Filters): FilterOption[] {
  return [
    ...filters.categories.map((value) => ({ group: "categories" as const, value })),
    ...filters.kinds.map((value) => ({ group: "kinds" as const, value })),
    ...filters.changed.map((value) => ({ group: "changed" as const, value })),
  ];
}

export function optionLabel(option: FilterOption): string {
  switch (option.group) {
    case "categories":
      return CATEGORY_LABEL[option.value];
    case "kinds":
      return KIND_LABEL[option.value];
    case "changed":
      return CHANGED_LABEL[option.value];
  }
}

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
