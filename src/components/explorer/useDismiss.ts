import { useEffect, useRef, type RefObject } from "react";

/** Escape, or a press outside both the panel and its button, closes the panel. */
export function useDismiss(
  panelRef: RefObject<HTMLElement | null>,
  anchorRef: RefObject<HTMLElement | null>,
  onClose: () => void,
  onDismiss: () => void,
) {
  const closeRef = useRef(onClose);
  const dismissRef = useRef(onDismiss);
  useEffect(() => {
    closeRef.current = onClose;
    dismissRef.current = onDismiss;
  });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        closeRef.current();
      }
    };
    const onPress = (e: PointerEvent) => {
      const target = e.target as Node;
      if (panelRef.current?.contains(target) || anchorRef.current?.contains(target)) return;
      dismissRef.current();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPress);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPress);
    };
  }, [anchorRef, panelRef]);
}
