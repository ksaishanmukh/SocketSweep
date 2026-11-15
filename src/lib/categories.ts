import type { Category, ChangedBucket, KindFilter } from "../api";

/** In the order the filter panel and legends list them. */
export const CATEGORIES: Category[] = ["photo", "video", "audio", "doc", "archive", "other"];

export const CATEGORY_LABEL: Record<Category, string> = {
  photo: "Photos",
  video: "Videos",
  audio: "Audio",
  doc: "Documents",
  archive: "Archives",
  other: "Other",
};

export const KINDS: KindFilter[] = ["folder", "file"];

export const KIND_LABEL: Record<KindFilter, string> = {
  folder: "Folders",
  file: "Files",
};

export const CHANGED_BUCKETS: ChangedBucket[] = [
  "last30d",
  "months1To6",
  "months6To12",
  "overYear",
];

export const CHANGED_LABEL: Record<ChangedBucket, string> = {
  last30d: "Last 30 days",
  months1To6: "1 to 6 months ago",
  months6To12: "6 to 12 months ago",
  overYear: "Over a year ago",
};

/** The category's solid colour, which follows the light or dark theme. */
export function categoryColor(category: Category): string {
  return `var(--cat-${category})`;
}

/**
 * A wash of the category over the panel. A wash rather than the solid colour
 * keeps the map calm and leaves the label in normal text colour readable.
 */
export function categoryTint(category: Category, percent = 26): string {
  return `color-mix(in srgb, var(--cat-${category}) ${percent}%, var(--panel))`;
}
