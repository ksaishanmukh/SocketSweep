/**
 * What the pointer is over, shared by the map and the list. Where it came
 * from matters: pointing at a row only hints at its tile, so the map stays
 * calm, while pointing at a tile lights it up and marks its row.
 */
export interface Hover {
  key: string;
  from: "list" | "map";
}

export type SetHover = (hover: Hover | null) => void;
