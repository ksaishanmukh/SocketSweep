import { useEffect, useRef, useState } from "react";
import type { View } from "../../api";
import type { ScanSession } from "../../hooks/useScanSession";
import type { Target } from "../../lib/selection";
import { filterCount } from "../../lib/viewOptions";
import type { Hover } from "./hover";
import { ListCard } from "./ListCard";
import { MapCard } from "./MapCard";

/** The open folder as a map above and a list below, which point at each other. */
export function ExplorerScreen({
  session,
  view,
  onDelete,
}: {
  session: ScanSession;
  view: View;
  onDelete: (targets: Target[]) => void;
}) {
  const [hover, setHover] = useState<Hover | null>(null);
  const { nav, options, actions, metric, scanInfo } = session;
  useBackForward(nav.back, nav.forward);
  const filtering = filterCount(options.filters) > 0 || options.folderQuery !== null;

  return (
    <div className="h-full max-w-[1320px] mx-auto px-6 pt-4 pb-4 flex flex-col min-h-0 overflow-y-auto">
      <MapCard
        treemap={nav.treemap}
        view={view}
        metric={metric}
        filtering={filtering}
        running={scanInfo?.running ?? false}
        hover={hover}
        onHover={setHover}
        onOpen={(node) => void actions.openFolder(node)}
      />
      <ListCard
        session={session}
        view={view}
        hover={hover}
        onHover={setHover}
        onDelete={onDelete}
      />
    </div>
  );
}

/** The mouse's side buttons and Alt+arrow keys step through the history, as in a browser. */
function useBackForward(back: () => Promise<void>, forward: () => Promise<void>) {
  const handlers = useRef({ back, forward });
  useEffect(() => {
    handlers.current = { back, forward };
  });
  useEffect(() => {
    const onMouse = (e: MouseEvent) => {
      if (e.button === 3) void handlers.current.back();
      else if (e.button === 4) void handlers.current.forward();
      else return;
      e.preventDefault();
    };
    const onKey = (e: KeyboardEvent) => {
      if (!e.altKey || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
      e.preventDefault();
      void (e.key === "ArrowLeft" ? handlers.current.back() : handlers.current.forward());
    };
    window.addEventListener("mouseup", onMouse);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mouseup", onMouse);
      window.removeEventListener("keydown", onKey);
    };
  }, []);
}
