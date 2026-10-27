import { useState } from "react";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ConsoleDrawer } from "./components/ConsoleDrawer";
import { FailedScreen } from "./components/screens/FailedScreen";
import { ResultScreen } from "./components/screens/ResultScreen";
import { ScanningScreen } from "./components/screens/ScanningScreen";
import { SetupScreen } from "./components/screens/SetupScreen";
import { Toasts } from "./components/Toasts";
import { useScanSession } from "./hooks/useScanSession";
import { useToasts } from "./hooks/useToasts";
import type { Row } from "./lib/types";
import "./App.css";

export default function App() {
  const { toasts, push, dismiss } = useToasts();
  const session = useScanSession(push);

  // `dir` is captured when the dialog opens, because the live view keeps changing under it.
  // So is the generation: a rescan or disconnect replaces the tree, and a
  // dialog still holding a row from the old one must not be shown, let alone
  // confirmed.
  const [request, setRequest] = useState<{ row: Row; dir?: string; generation: number } | null>(
    null,
  );
  const [consoleOpen, setConsoleOpen] = useState(false);

  const confirming = request?.generation === session.generation ? request : null;
  const requestDelete = (row: Row) =>
    setRequest({ row, dir: row.parent ?? session.view?.path, generation: session.generation });

  // Everything behind the dialog is inert so focus and clicks cannot leave it.
  const inert = confirming !== null;

  return (
    <div className="h-screen flex flex-col bg-surface-0 overflow-hidden">
      <Toasts toasts={toasts} onDismiss={dismiss} />

      <main className="flex-1 flex flex-col min-h-0" inert={inert}>
        {(session.phase === "setup" || session.phase === "connecting") && (
          <SetupScreen onConnect={session.connect} loading={session.phase === "connecting"} />
        )}

        {session.phase === "scanning" && (
          <ScanningScreen stats={session.stats} current={session.scanningPath} />
        )}

        {session.phase === "failed" && (
          <FailedScreen
            message={session.failure}
            onRescan={session.scan}
            onDisconnect={session.disconnect}
          />
        )}

        {session.phase === "result" && session.view && session.stats && (
          <ResultScreen
            view={session.view}
            crumbs={session.crumbs}
            stats={session.stats}
            treemap={session.treemap}
            mode={session.mode}
            largest={session.largest}
            types={session.types}
            apps={session.apps}
            previous={session.previous}
            onModeChange={session.setMode}
            onReveal={session.reveal}
            elapsedMs={session.elapsedMs}
            query={session.query}
            searchResults={session.searchResults}
            onQueryChange={session.setQuery}
            onOpen={session.open}
            onLoadMore={session.loadMore}
            loadingMore={session.loadingMore}
            onRescan={session.scan}
            onDisconnect={session.disconnect}
            onDelete={requestDelete}
          />
        )}
      </main>

      <div inert={inert}>
        <ConsoleDrawer
          logs={session.logs}
          open={consoleOpen}
          onToggle={() => setConsoleOpen((v) => !v)}
        />
      </div>

      {confirming && (
        <ConfirmDialog
          key={confirming.row.id}
          target={confirming.row}
          dir={confirming.dir}
          onCancel={() => setRequest(null)}
          onConfirm={() => {
            const target = confirming.row;
            setRequest(null);
            void session.remove(target);
          }}
        />
      )}
    </div>
  );
}
