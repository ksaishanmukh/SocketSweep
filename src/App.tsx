import { useState } from "react";
import { ConsoleDrawer } from "./components/ConsoleDrawer";
import { CopyBar } from "./components/CopyBar";
import { FailedScreen } from "./components/screens/FailedScreen";
import { ResultScreen } from "./components/screens/ResultScreen";
import { SetupScreen } from "./components/screens/setup/SetupScreen";
import { ActionBar } from "./components/selection/ActionBar";
import { DeleteDialog } from "./components/selection/DeleteDialog";
import { Toasts } from "./components/Toasts";
import { UpdateNotice } from "./components/UpdateNotice";
import { useAppInfo } from "./hooks/useAppInfo";
import { useScanSession, type ScanSession } from "./hooks/useScanSession";
import { useToasts } from "./hooks/useToasts";
import { inSession, type Target } from "./lib/selection";
import "./App.css";

export default function App() {
  const { toasts, push, dismiss } = useToasts();
  const session = useScanSession(push);
  const { version, update, dismissUpdate } = useAppInfo();

  // What the dialog lists is captured when it opens, because the live view
  // keeps changing under it. Its nodes carry their session: a rescan or
  // disconnect replaces the tree, and a dialog still holding nodes of the old
  // one must not be shown, let alone confirmed.
  const [request, setRequest] = useState<Target[] | null>(null);
  const [consoleOpen, setConsoleOpen] = useState(false);

  const confirming =
    request && request.every((t) => inSession(t, session.sessionId)) ? request : null;

  // Everything behind the dialog is inert so focus and clicks cannot leave it.
  const inert = confirming !== null;

  return (
    <div className="h-screen flex flex-col bg-bg overflow-hidden">
      <Toasts toasts={toasts} onDismiss={dismiss} />

      {update && <UpdateNotice release={update} onDismiss={dismissUpdate} />}

      <main className="flex-1 flex flex-col min-h-0" inert={inert}>
        <Screens session={session} version={version} onDelete={setRequest} />
      </main>

      <div inert={inert}>
        {session.phase === "browsing" && session.selection.items.length > 0 && (
          <ActionBar
            items={session.selection.items}
            metric={session.metric}
            copyDestination={session.copyDestination}
            copyBusy={session.copy !== null}
            onClear={session.selection.clear}
            onCopy={() => void session.startCopy(session.selection.items)}
            onDelete={() => setRequest(session.selection.items)}
          />
        )}
        {session.copy && <CopyBar copy={session.copy} onCancel={session.cancelCopy} />}
        <ConsoleDrawer
          logs={session.logs}
          version={version}
          open={consoleOpen}
          onToggle={() => setConsoleOpen((v) => !v)}
        />
      </div>

      {confirming && (
        <DeleteDialog
          targets={confirming}
          onCancel={() => setRequest(null)}
          onConfirm={() => {
            setRequest(null);
            void session.remove(confirming);
          }}
        />
      )}
    </div>
  );
}

function Screens({
  session,
  version,
  onDelete,
}: {
  session: ScanSession;
  version: string | null;
  onDelete: (targets: Target[]) => void;
}) {
  if (session.phase === "setup" || session.phase === "connecting") {
    return (
      <SetupScreen
        onConnect={session.connect}
        loading={session.phase === "connecting"}
        status={session.status}
        version={version}
      />
    );
  }
  if (session.phase === "failed") {
    return (
      <FailedScreen
        message={session.failure}
        onRescan={session.scan}
        onDisconnect={session.disconnect}
      />
    );
  }
  const view = session.nav.view;
  return view ? <ResultScreen session={session} view={view} onDelete={onDelete} /> : null;
}
