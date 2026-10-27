import { useState } from "react";
import type { Row } from "./api";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ConsoleDrawer } from "./components/ConsoleDrawer";
import { CopyBar } from "./components/CopyBar";
import { FailedScreen } from "./components/screens/FailedScreen";
import { ResultScreen } from "./components/screens/ResultScreen";
import { SetupScreen } from "./components/screens/SetupScreen";
import { Toasts } from "./components/Toasts";
import { useScanSession } from "./hooks/useScanSession";
import { useToasts } from "./hooks/useToasts";
import { joinPath } from "./lib/path";
import { nodeKey } from "./lib/size";
import "./App.css";

export default function App() {
  const { toasts, push, dismiss } = useToasts();
  const session = useScanSession(push);

  // The full path is captured when the dialog opens, because the live view
  // keeps changing under it. So is the row's session (in its NodeRef): a rescan
  // or disconnect replaces the tree, and a dialog still holding a row from the
  // old one must not be shown, let alone confirmed.
  const [request, setRequest] = useState<{ row: Row; path: string } | null>(null);
  const [consoleOpen, setConsoleOpen] = useState(false);

  const confirming = request?.row.node.session === session.sessionId ? request : null;
  const requestDelete = (row: Row) =>
    setRequest({ row, path: joinPath(row.parent ?? session.view?.path ?? "", row.name) });

  // Everything behind the dialog is inert so focus and clicks cannot leave it.
  const inert = confirming !== null;

  return (
    <div className="h-screen flex flex-col bg-surface-0 overflow-hidden">
      <Toasts toasts={toasts} onDismiss={dismiss} />

      <main className="flex-1 flex flex-col min-h-0" inert={inert}>
        {(session.phase === "setup" || session.phase === "connecting") && (
          <SetupScreen
            onConnect={session.connect}
            loading={session.phase === "connecting"}
            status={session.status}
          />
        )}

        {session.phase === "failed" && (
          <FailedScreen
            message={session.failure}
            onRescan={session.scan}
            onDisconnect={session.disconnect}
          />
        )}

        {session.phase === "browsing" && session.view && (
          <ResultScreen session={session} view={session.view} onDelete={requestDelete} />
        )}
      </main>

      <div inert={inert}>
        {session.copy && <CopyBar copy={session.copy} onCancel={session.cancelCopy} />}
        <ConsoleDrawer
          logs={session.logs}
          open={consoleOpen}
          onToggle={() => setConsoleOpen((v) => !v)}
        />
      </div>

      {confirming && (
        <ConfirmDialog
          key={nodeKey(confirming.row.node)}
          target={confirming.row}
          path={confirming.path}
          onCancel={() => setRequest(null)}
          onConfirm={() => {
            const { row, path } = confirming;
            setRequest(null);
            void session.remove(row, path);
          }}
        />
      )}
    </div>
  );
}
