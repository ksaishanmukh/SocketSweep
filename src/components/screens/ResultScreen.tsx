import type { Row, View } from "../../api";
import type { ScanSession } from "../../hooks/useScanSession";
import { ScanStatus } from "../ScanStatus";
import { MainCanvas } from "./result/MainCanvas";
import { ResultHeader } from "./result/ResultHeader";
import { SidePanel } from "./result/SidePanel";
import { TotalsBar } from "./result/TotalsBar";

export function ResultScreen({
  session,
  view,
  onDelete,
}: {
  session: ScanSession;
  /** The open folder; present whenever this screen is shown. */
  view: View;
  onDelete: (row: Row) => void;
}) {
  const { scanInfo } = session;
  const running = scanInfo?.running ?? false;

  return (
    <div className="flex flex-col h-full animate-fade-in-up min-h-0">
      <ResultHeader session={session} running={running} />
      <TotalsBar session={session} running={running} />

      {scanInfo && (
        <ScanStatus
          scan={scanInfo}
          onStop={session.cancelScan}
          onRescan={() => void session.scan()}
        />
      )}

      {/* The treemap is the point of the screen, so it gets the room. */}
      <div className="flex-1 min-h-0 flex flex-col lg:flex-row gap-3 p-3">
        <MainCanvas session={session} view={view} running={running} onDelete={onDelete} />
        <SidePanel session={session} view={view} onDelete={onDelete} />
      </div>
    </div>
  );
}
