import { IconUsb, Spinner } from "../icons";

export function SetupScreen({ onConnect, loading }: { onConnect: () => void; loading: boolean }) {
  return (
    <div className="flex flex-col items-center justify-center flex-1 gap-8 animate-fade-in-up">
      <div className="flex flex-col items-center gap-6">
        <div
          className={`w-24 h-24 rounded-2xl bg-gradient-to-br from-accent-500/20 to-accent-700/10
            border border-accent-500/20 flex items-center justify-center
            ${loading ? "animate-pulse-glow" : ""}`}
        >
          <span className="text-accent-400">
            <IconUsb />
          </span>
        </div>

        <div className="text-center">
          <h1 className="text-3xl font-bold tracking-tight bg-gradient-to-r from-zinc-100 to-zinc-400 bg-clip-text text-transparent">
            SocketSweep
          </h1>
          <p className="mt-2 text-sm text-zinc-500 max-w-xs leading-relaxed">
            High-performance Android storage analyzer.
            <br />
            <span className="text-zinc-600">Bypasses MTP — direct POSIX scanning.</span>
          </p>
        </div>
      </div>

      <button
        onClick={onConnect}
        disabled={loading}
        className={`px-8 py-3 rounded-xl font-semibold text-sm transition-all duration-300
          focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent-400
          ${
            loading
              ? "bg-zinc-800 text-zinc-500 cursor-wait"
              : "bg-gradient-to-r from-accent-600 to-accent-500 text-white cursor-pointer hover:from-accent-500 hover:to-accent-400 hover:shadow-lg hover:shadow-accent-500/20 hover:-translate-y-0.5"
          }`}
      >
        {loading ? (
          <span className="flex items-center gap-2">
            <Spinner size={16} />
            Connecting…
          </span>
        ) : (
          <span className="flex items-center gap-2">
            <IconUsb />
            Connect Device
          </span>
        )}
      </button>

      {/* ADB ships with the app, so it is deliberately absent from this list. */}
      <div className="flex flex-col items-center gap-1.5 text-xs text-zinc-600">
        <p>• Android device connected via USB</p>
        <p>• USB Debugging enabled in Developer Options</p>
      </div>
    </div>
  );
}
