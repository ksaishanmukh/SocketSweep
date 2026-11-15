import { useCallback, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import {
  keyHeight,
  moveDrag,
  readStoredHeight,
  releaseDrag,
  startDrag,
  storeHeight,
  type Drag,
} from "../../lib/mapGrip";

/**
 * The map's height and the grip's handlers. The rules live in `mapGrip`;
 * this only feeds them pointer and key events and remembers the result.
 */
export function useMapHeight() {
  const [height, setHeight] = useState(readStoredHeight);
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{ state: Drag; y: number } | null>(null);
  const lastOpen = useRef(height > 0 ? height : 0);

  const settle = useCallback((next: number) => {
    if (next > 0) lastOpen.current = next;
    setHeight(next);
    storeHeight(next);
  }, []);

  const onPointerDown = useCallback(
    (e: PointerEvent<HTMLElement>) => {
      if (e.button !== 0) return;
      e.currentTarget.setPointerCapture?.(e.pointerId);
      drag.current = { state: startDrag(height), y: e.clientY };
      setDragging(true);
    },
    [height],
  );

  const onPointerMove = useCallback((e: PointerEvent<HTMLElement>) => {
    const d = drag.current;
    if (!d) return;
    d.state = moveDrag(d.state, e.clientY - d.y);
    setHeight(d.state.height);
  }, []);

  const onPointerUp = useCallback(() => {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    setDragging(false);
    settle(releaseDrag(d.state, lastOpen.current));
  }, [settle]);

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLElement>) => {
      const next = keyHeight(e.key, height, lastOpen.current);
      if (next === null) return;
      e.preventDefault();
      settle(next);
    },
    [height, settle],
  );

  return {
    height,
    dragging,
    grip: { onPointerDown, onPointerMove, onPointerUp, onPointerCancel: onPointerUp, onKeyDown },
  };
}
