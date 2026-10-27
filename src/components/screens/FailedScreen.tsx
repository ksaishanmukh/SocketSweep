/**
 * A scan that did not finish.
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
    <div className="flex flex-col items-center justify-center flex-1 gap-6 animate-fade-in-up px-6 text-center">
      <div className="max-w-md">
        <h2 className="text-lg font-semibold text-zinc-200" role="alert">
          The scan did not finish
        </h2>
        {message && <p className="mt-2 text-sm text-zinc-500 leading-relaxed">{message}</p>}
        <p className="mt-2 text-xs text-zinc-600">
          Partial results are not shown, because they would leave out part of the device.
        </p>
      </div>

      <div className="flex items-center gap-3">
        <button
          onClick={onRescan}
          className="px-6 py-2.5 rounded-xl font-semibold text-sm text-white bg-gradient-to-r from-accent-600 to-accent-500
            hover:from-accent-500 hover:to-accent-400 focus-visible:outline-2 focus-visible:outline-offset-2
            focus-visible:outline-accent-400"
        >
          ↻ Rescan
        </button>
        <button
          onClick={onDisconnect}
          className="px-4 py-2.5 rounded-xl text-sm font-medium bg-zinc-800 text-zinc-400
            hover:bg-red-950/50 hover:text-red-400 border border-zinc-700/50
            focus-visible:outline-2 focus-visible:outline-accent-500"
        >
          Disconnect
        </button>
      </div>
    </div>
  );
}
