/**
 * A scan that never produced anything to browse.
 *
 * Nothing from the partial walk is shown: its totals and folders would be
 * missing an unknown share of the device. The phone is still connected, so the
 * useful actions are to try again or to let go of it.
 */
export function FailedScreen({
  message,
  onRescan,
  onDisconnect,
}: {
  message: string | null;
  onRescan: () => void;
  onDisconnect: () => void;
}) {
  return (
    <div className="flex flex-col items-center justify-center flex-1 gap-6 rise-in px-6 text-center">
      <div className="max-w-md">
        <h2 className="text-lg font-semibold text-fg" role="alert">
          The scan did not finish
        </h2>
        {message && <p className="mt-2 text-sm text-muted leading-relaxed">{message}</p>}
        <p className="mt-2 text-xs text-faint">
          Nothing was listed before it stopped, so there is nothing to browse yet.
        </p>
      </div>

      <div className="flex items-center gap-3">
        <button
          onClick={onRescan}
          className="px-6 py-2.5 rounded-xl font-semibold text-sm bg-accent text-on-accent hover:opacity-90"
        >
          ↻ Rescan
        </button>
        <button
          onClick={onDisconnect}
          className="px-4 py-2.5 rounded-xl text-sm font-medium bg-sunk text-muted
            hover:bg-sunk hover:text-danger border border-line"
        >
          Disconnect
        </button>
      </div>
    </div>
  );
}
