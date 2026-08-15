import { useState } from "react";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ConsoleDrawer } from "./components/ConsoleDrawer";
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

  const [confirming, setConfirming] = useState<Row | null>(null);
  const [consoleOpen, setConsoleOpen] = useState(false);

  return (
    <div className="h-screen flex flex-col bg-surface-0 overflow-hidden">
      <Toasts toasts={toasts} onDismiss={dismiss} />

      <main className="flex-1 flex flex-col min-h-0">
        {(session.phase === "setup" || session.phase === "connecting") && (
          <SetupScreen onConnect={session.connect} loading={session.phase === "connecting"} />
        )}

        {session.phase === "scanning" && (
          <ScanningScreen stats={session.stats} current={session.scanningPath} />
        )}

        {session.phase === "result" && session.view && session.stats && (
          <ResultScreen
            view={session.view}
            crumbs={session.crumbs}
            stats={session.stats}
            treemap={session.treemap}
            elapsedMs={session.elapsedMs}
            query={session.query}
            searchResults={session.searchResults}
            onQueryChange={session.setQuery}
            onOpen={session.open}
            onRescan={session.scan}
            onDisconnect={session.disconnect}
            onDelete={setConfirming}
          />
        )}
      </main>

      <ConsoleDrawer
        logs={session.logs}
        open={consoleOpen}
        onToggle={() => setConsoleOpen((v) => !v)}
      />

      {confirming && (
        <ConfirmDialog
          target={confirming}
          onCancel={() => setConfirming(null)}
          onConfirm={() => {
            const target = confirming;
            setConfirming(null);
            void session.remove(target);
          }}
        />
      )}
    </div>
  );
}
