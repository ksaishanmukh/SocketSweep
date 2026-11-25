import { useEffect, useRef, type RefObject } from "react";

/** Somewhere a person types: a key there is text, not a shortcut. */
function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable || target.tagName === "TEXTAREA") return true;
  if (target.tagName !== "INPUT") return false;
  return !["checkbox", "radio", "button"].includes((target as HTMLInputElement).type);
}

/**
 * Ctrl/Cmd+F focuses the search box and `?` opens the shortcuts, anywhere in
 * the Explorer. Skipped while a dialog has made the screen inert, and `?`
 * while typing.
 */
export function useExplorerShortcuts(
  searchRef: RefObject<HTMLInputElement | null>,
  openHelp: () => void,
) {
  const help = useRef(openHelp);
  useEffect(() => {
    help.current = openHelp;
  });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const search = searchRef.current;
      if (!search || search.closest("[inert]")) return;
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        search.focus();
        search.select();
      } else if (e.key === "?" && !e.ctrlKey && !e.metaKey && !e.altKey && !isTyping(e.target)) {
        e.preventDefault();
        help.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [searchRef]);
}
