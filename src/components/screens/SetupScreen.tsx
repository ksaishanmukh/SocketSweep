import type { Device, DeviceState } from "../../api";
import { useDevices } from "../../hooks/useDevices";
import { IconUsb, Spinner } from "../icons";

/** How each non-usable state is named, and what to do about it, since the fix differs. */
const TROUBLE: Record<Exclude<DeviceState, "ready">, { name: string; fix?: string }> = {
  unauthorized: { name: "waiting for permission", fix: "Tap “Allow USB debugging” on the phone" },
  offline: { name: "offline", fix: "Unplug and reconnect the cable" },
  noPermission: { name: "blocked", fix: "The system is blocking access to this device" },
  other: { name: "not ready" },
};

export function SetupScreen({
  onConnect,
  loading,
  status = "Connecting…",
}: {
  onConnect: (serial?: string) => void;
  loading: boolean;
  /** What the host is doing while `loading`: connecting, then starting the scan. */
  status?: string;
}) {
  const { devices, problem } = useDevices(loading);

  const usable = devices?.filter((d) => d.usable) ?? [];
  const problems = devices?.filter((d) => !d.usable) ?? [];
  // With one device the button is enough; a picker would be ceremony.
  const showPicker = usable.length > 1;

  return (
    <div className="flex flex-col items-center justify-center flex-1 gap-8 animate-fade-in-up px-6">
      <Hero loading={loading} />

      {/* With more than one device attached, the choice has to be explicit. */}
      {showPicker && !loading ? (
        <DevicePicker devices={usable} onConnect={onConnect} />
      ) : (
        <ConnectButton
          loading={loading}
          status={status}
          usable={usable}
          onConnect={() => onConnect()}
        />
      )}

      {!loading && (
        <>
          <DeviceProblems devices={problems} />
          {problem && (
            <p role="alert" className="max-w-sm text-center text-xs text-amber-400/80">
              {problem}
            </p>
          )}
          {usable.length === 0 && problems.length === 0 && !problem && <Prerequisites />}
        </>
      )}
    </div>
  );
}

function troubleOf(state: DeviceState): { name: string; fix?: string } {
  return state === "ready" ? { name: "not usable" } : TROUBLE[state];
}

function Hero({ loading }: { loading: boolean }) {
  return (
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
  );
}

function DevicePicker({
  devices,
  onConnect,
}: {
  devices: Device[];
  onConnect: (serial: string) => void;
}) {
  return (
    <div className="w-full max-w-sm flex flex-col gap-2" role="group" aria-label="Choose a device">
      <p className="text-xs text-zinc-500 text-center mb-1">
        {devices.length} devices connected — pick one
      </p>
      {devices.map((d) => (
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
            <span className="block text-[11px] font-mono text-zinc-600 truncate">{d.serial}</span>
            {d.hint && <span className="block mt-0.5 text-[11px] text-zinc-500">{d.hint}</span>}
          </span>
        </button>
      ))}
    </div>
  );
}

function ConnectButton({
  loading,
  status,
  usable,
  onConnect,
}: {
  loading: boolean;
  status: string;
  usable: Device[];
  onConnect: () => void;
}) {
  return (
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
          {status}
        </span>
      ) : (
        <span className="flex items-center gap-2">
          <IconUsb />
          {usable.length === 1 ? `Scan ${usable[0].model}` : "Connect Device"}
        </span>
      )}
    </button>
  );
}

/** Attached but unusable: say which problem it is, not just "not found". */
function DeviceProblems({ devices }: { devices: Device[] }) {
  if (devices.length === 0) return null;
  return (
    <ul className="flex flex-col gap-1.5 text-xs">
      {devices.map((d) => {
        const trouble = troubleOf(d.state);
        return (
          <li key={d.serial} className="text-amber-400/80">
            <span className="font-mono">{d.model}</span> is {trouble.name}
            {trouble.fix && <span className="text-zinc-500"> — {trouble.fix}</span>}
            {d.hint && <p className="mt-0.5 text-[11px] text-zinc-500">{d.hint}</p>}
          </li>
        );
      })}
    </ul>
  );
}

/** ADB ships with the app, so it is deliberately absent from this list. */
function Prerequisites() {
  return (
    <div className="flex flex-col items-center gap-1.5 text-xs text-zinc-600">
      <p>• Android device connected via USB</p>
      <p>• USB Debugging enabled in Developer Options</p>
    </div>
  );
}
