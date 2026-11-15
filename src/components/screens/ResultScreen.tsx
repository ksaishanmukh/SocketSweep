import { useEffect, useRef } from "react";
import type { View } from "../../api";
import type { ScanSession } from "../../hooks/useScanSession";
import type { Target } from "../../lib/selection";
import { IconStoreContext } from "../common/iconContext";
import { ExplorerScreen } from "../explorer/ExplorerScreen";
import { OverviewScreen } from "../overview/OverviewScreen";
import { UNREADABLE_CARD_ID } from "../overview/UnreadableCard";
import { AppHeader } from "../shell/AppHeader";

/** The scanned phone: the header and totals, then the tab the user picked. */
export function ResultScreen({
  session,
  view,
  onDelete,
}: {
  session: ScanSession;
  /** The open folder; present whenever this screen is shown. */
  view: View;
  onDelete: (targets: Target[]) => void;
}) {
  const showUnreadable = useJumpToUnreadable(session);
  return (
    <IconStoreContext.Provider value={session.icons}>
      <div className="flex flex-col h-full min-h-0">
        <AppHeader session={session} onShowUnreadable={showUnreadable} />
        <div
          role="tabpanel"
          aria-label={session.mode === "overview" ? "Overview" : "Explorer"}
          className="flex-1 min-h-0 overflow-y-auto"
        >
          {session.mode === "overview" ? (
            <OverviewScreen session={session} />
          ) : (
            <ExplorerScreen session={session} view={view} onDelete={onDelete} />
          )}
        </div>
      </div>
    </IconStoreContext.Provider>
  );
}

/** The totals line's warning link: over to the Overview, then to the card that explains it. */
function useJumpToUnreadable(session: ScanSession) {
  const pending = useRef(false);
  const { mode, actions } = session;

  // From the Explorer the card exists only once the Overview has rendered, so
  // the jump waits for that render.
  useEffect(() => {
    if (pending.current && mode === "overview" && jumpToUnreadable()) pending.current = false;
  });

  return () => {
    if (mode === "overview" && jumpToUnreadable()) return;
    pending.current = true;
    actions.setMode("overview");
  };
}

function jumpToUnreadable(): boolean {
  const card = document.getElementById(UNREADABLE_CARD_ID);
  if (!card) return false;
  card.scrollIntoView?.({ behavior: "smooth", block: "start" });
  document.getElementById(`${UNREADABLE_CARD_ID}-title`)?.focus();
  return true;
}
