import { useEffect, useState, type RefObject } from "react";

/** The element's content size in whole pixels, kept current as it resizes. */
export function useElementSize(elementRef: RefObject<HTMLElement | null>) {
  const [size, setSize] = useState({ w: 0, h: 0 });

  // Track the container so the layout recomputes on resize.
  useEffect(() => {
    const el = elementRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      setSize({ w: Math.floor(width), h: Math.floor(height) });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [elementRef]);

  return size;
}
