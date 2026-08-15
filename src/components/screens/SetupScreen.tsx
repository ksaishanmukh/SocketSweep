import { useEffect, useState } from "react";
import * as ipc from "../../lib/ipc";
import type { Device } from "../../lib/types";
import { IconUsb, Spinner } from "../icons";

/** How each non-usable state is explained, since the fix differs. */
const TROUBLE: Record<string, string> = {
  unauthorized: "Tap “Allow USB debugging” on the phone",
  offline: "Unplug and reconnect the cable",
  authorizing: "Waiting for the phone to authorise this computer",
  noperm: "The system is blocking access to this device",
};

/** Fast enough that plugging in feels instant, slow enough to be free. */
const DEVICE_POLL_MS = 2000;

export function SetupScreen({
  onConnect,
  loading,
}: {
  onConnect: (serial?: string) => void;
  loading: boolean;
}) {
  const [devices, setDevices] = useState<Device[] | null>(null);

  // Poll while idle so plugging a phone in is noticed without a manual refresh.
  // The cancelled flag matters: a poll in flight when this screen unmounts (the
  // usual case, since connecting navigates away) must not set state afterwards.
  useEffect(() => {
    if (loading) return;
    let cancelled = false;

    const poll = async () => {
      try {
        const list = await ipc.listDevices();
        if (!cancelled) setDevices(list);
      } catch {
        // No adb server means nothing to list. Connect reports the real error.
        if (!cancelled) setDevices([]);
      }
    };

    void poll();
    const timer = setInterval(poll, DEVICE_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [loading]);

  const usable = devices?.filter((d) => d.usable) ?? [];
  const problems = devices?.filter((d) => !d.usable) ?? [];
  // With one device the button is enough; a picker would be ceremony.
  const showPicker = usable.length > 1;

  return (
    <div className="flex flex-col items-center justify-center flex-1 gap-8 animate-fade-in-up px-6">
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

      {/* With more than one device attached, the choice has to be explicit. */}
      {showPicker && !loading ? (
        <div
          className="w-full max-w-sm flex flex-col gap-2"
          role="group"
          aria-label="Choose a device"
        >
          <p className="text-xs text-zinc-500 text-center mb-1">
            {usable.length} devices connected — pick one
          </p>
          {usable.map((d) => (
            <button
              key={d.serial}
              onClick={() => onConnect(d.serial)}
              className="flex items-center gap-3 px-4 py-3 rounded-xl bg-zinc-900 border border-zinc-800
                hover:border-accent-500/40 hover:bg-zinc-800/60 text-left transition-colors
                focus-visible:outline-2 focus-visible:outline-accent-500"
            >
              <span className="text-accent-400 shrink-0">
                <IconUsb />
              </span>
              <span className="min-w-0 flex-1">
                <span className="block text-sm font-medium text-zinc-200 truncate">{d.model}</span>
                <span className="block text-[11px] font-mono text-zinc-600 truncate">
                  {d.serial}
                </span>
              </span>
            </button>
          ))}
        </div>
      ) : (
        <button
          onClick={() => onConnect()}
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
              {usable.length === 1 ? `Scan ${usable[0].model}` : "Connect Device"}
            </span>
          )}
        </button>
      )}

      {/* Attached but unusable: say which problem it is, not just "not found". */}
      {!loading && problems.length > 0 && (
        <ul className="flex flex-col gap-1.5 text-xs">
          {problems.map((d) => (
            <li key={d.serial} className="text-amber-400/80">
              <span className="font-mono">{d.model}</span> is {d.state}
              {TROUBLE[d.state] && <span className="text-zinc-500"> — {TROUBLE[d.state]}</span>}
            </li>
          ))}
        </ul>
      )}

      {/* ADB ships with the app, so it is deliberately absent from this list. */}
      {!loading && usable.length === 0 && problems.length === 0 && (
        <div className="flex flex-col items-center gap-1.5 text-xs text-zinc-600">
          <p>• Android device connected via USB</p>
          <p>• USB Debugging enabled in Developer Options</p>
        </div>
      )}
    </div>
  );
}
