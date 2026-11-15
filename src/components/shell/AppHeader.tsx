import type { ScanSession } from "../../hooks/useScanSession";
import { Segmented } from "../common/Segmented";
import { IconPhone, IconRescan } from "../icons";
import { TotalsLine } from "./TotalsLine";

const TABS = [
  { value: "overview" as const, label: "Overview" },
  { value: "explorer" as const, label: "Explorer" },
];

/** The device, the two tabs and the session's actions, with the totals line under them. */
export function AppHeader({
  session,
  onShowUnreadable,
}: {
  session: ScanSession;
  onShowUnreadable: () => void;
}) {
  return (
    <header className="shrink-0 bg-panel border-b border-line">
      <div className="flex flex-wrap items-center gap-4 px-6 py-2.5">
        <Device model={session.model} root={session.rootPath} />
        <div className="flex-1 flex justify-center">
          <Segmented
            role="tablist"
            label="View"
            options={TABS}
            value={session.mode}
            onChange={session.actions.setMode}
          />
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void session.scan()}
            className="min-h-9 px-3 rounded-lg border border-line text-[13px] font-medium hover:bg-sunk inline-flex items-center gap-1.5"
          >
            <IconRescan />
            Rescan
          </button>
          <button
            type="button"
            onClick={() => void session.disconnect()}
            className="min-h-9 px-3 rounded-lg border border-line text-[13px] font-medium hover:bg-sunk"
          >
            Disconnect
          </button>
        </div>
      </div>
      <TotalsLine session={session} onShowUnreadable={onShowUnreadable} />
    </header>
  );
}

function Device({ model, root }: { model: string | null; root: string | null }) {
  return (
    <div className="flex items-center gap-3 min-w-0">
      <span className="w-9 h-9 rounded-lg bg-sunk border border-line flex items-center justify-center text-muted shrink-0">
        <IconPhone />
      </span>
      <div className="min-w-0">
        <div className="text-[14px] font-semibold truncate">{model ?? "Phone"}</div>
        <div className="text-[12px] text-muted truncate">
          <span className="text-ok" aria-hidden>
            ●
          </span>{" "}
          Connected over USB{root ? ` · ${root}` : ""}
        </div>
      </div>
    </div>
  );
}
