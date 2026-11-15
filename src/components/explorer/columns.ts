import type { SortKey } from "../../api";

/** Checkbox | name | size | share | contents | last changed | actions, shared by the header and every row. */
export const ROW_GRID = "28px minmax(0,1fr) 88px minmax(90px,170px) 110px 110px 76px";

/** Tall enough for a two-line app row and for a comfortable click. */
export const ROW_HEIGHT = 44;

/** The sortable columns, in order. Share sorts by size, since share is size over the same folder. */
export const COLUMNS: { label: string; key: SortKey; align: "left" | "right" }[] = [
  { label: "Name", key: "name", align: "left" },
  { label: "Size", key: "size", align: "right" },
  { label: "Share of this folder", key: "size", align: "left" },
  { label: "Contents", key: "count", align: "left" },
  { label: "Last changed", key: "changed", align: "left" },
];
