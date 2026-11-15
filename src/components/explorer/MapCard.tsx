import { useMemo, useRef } from "react";
import type { NodeRef, SizeMetric, TreemapNode, View } from "../../api";
import { useElementSize } from "../../hooks/useElementSize";
import { MAP_MAX_HEIGHT } from "../../lib/mapGrip";
import { fadedTiles, mapItems } from "../../lib/mapTiles";
import { squarify } from "../../lib/squarify";
import type { Hover, SetHover } from "./hover";
import { MapTile } from "./MapTile";
import { useMapHeight } from "./useMapHeight";

/**
 * The open folder's children as a squarified map, in a card of its own, with
 * a grip under it that resizes it, down to nothing but the grip.
 */
export function MapCard(props: {
  treemap: TreemapNode | null;
  view: View;
  metric: SizeMetric;
  filtering: boolean;
  running: boolean;
  hover: Hover | null;
  onHover: SetHover;
  onOpen: (node: NodeRef) => void;
}) {
  const { height, dragging, grip } = useMapHeight();
  return (
    <div className="shrink-0">
      <div
        className="map-frame card overflow-hidden"
        data-dragging={dragging}
        style={{ height, borderWidth: height === 0 ? 0 : undefined }}
      >
        {height > 0 && <MapTiles {...props} />}
      </div>
      <div
        role="separator"
        aria-orientation="horizontal"
        aria-label="Map height. Drag, or use the arrow keys; Enter hides or shows the map"
        aria-valuemin={0}
        aria-valuemax={MAP_MAX_HEIGHT}
        aria-valuenow={height}
        tabIndex={0}
        data-dragging={dragging}
        className="map-grip flex items-center justify-center h-4 my-0.5 rounded"
        {...grip}
      >
        <span className="block w-11 h-1 rounded-full bg-line" />
      </div>
    </div>
  );
}

function MapTiles({
  treemap,
  view,
  metric,
  filtering,
  running,
  hover,
  onHover,
  onOpen,
}: Parameters<typeof MapCard>[0]) {
  const hostRef = useRef<HTMLDivElement>(null);
  const box = useElementSize(hostRef);
  const { items, byId } = useMemo(() => mapItems(treemap, metric), [treemap, metric]);
  const tiles = useMemo(() => squarify(items, box.w, box.h), [items, box.w, box.h]);
  const faded = useMemo(() => fadedTiles(view, filtering), [view, filtering]);

  return (
    <div ref={hostRef} className="relative w-full h-full" role="group" aria-label="Folder map">
      {tiles.length === 0 && (
        <p className="absolute inset-0 flex items-center justify-center text-[13px] text-muted">
          {running && view.measure !== "final" ? "Measuring this folder…" : "Nothing to show here."}
        </p>
      )}
      {tiles.map((tile) => {
        const node = byId.get(tile.id);
        if (!node) return null;
        return (
          <MapTile
            key={tile.id}
            tile={tile}
            node={node}
            metric={metric}
            faded={faded(node.node)}
            hover={hover}
            onHover={onHover}
            onOpen={(n) => onOpen(n.node)}
          />
        );
      })}
    </div>
  );
}
