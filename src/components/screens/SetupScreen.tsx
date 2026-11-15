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
    <div className="flex flex-col items-center justify-center flex-1 gap-8 rise-in px-6">
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
            <p role="alert" className="max-w-sm text-center text-xs text-warn">
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
        className={`w-24 h-24 rounded-2xl bg-panel border border-line flex items-center justify-center
            ${loading ? "animate-pulse" : ""}`}
      >
        <span className="text-accent">
          <IconUsb />
        </span>
      </div>

      <div className="text-center">
        <h1 className="text-3xl font-bold tracking-tight text-fg">SocketSweep</h1>
        <p className="mt-2 text-sm text-muted max-w-xs leading-relaxed">
          High-performance Android storage analyzer.
          <br />
          <span className="text-faint">Bypasses MTP — direct POSIX scanning.</span>
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
      <p className="text-xs text-muted text-center mb-1">
        {devices.length} devices connected — pick one
      </p>
      {devices.map((d) => (
        <button
          key={d.serial}
          onClick={() => onConnect(d.serial)}
          className="flex items-center gap-3 px-4 py-3 rounded-xl bg-panel border border-line
                hover:border-accent hover:bg-sunk text-left transition-colors"
        >
          <span className="text-accent shrink-0">
            <IconUsb />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block text-sm font-medium text-fg truncate">{d.model}</span>
            <span className="block text-[11px] font-mono text-faint truncate">{d.serial}</span>
            {d.hint && <span className="block mt-0.5 text-[11px] text-muted">{d.hint}</span>}
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
      type="button"
      onClick={onConnect}
      disabled={loading}
      className={`px-8 py-3 rounded-xl font-semibold text-sm transition-all duration-300
            ${
              loading
                ? "bg-sunk text-muted cursor-wait"
                : "bg-accent text-on-accent cursor-pointer hover:opacity-90"
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
          <li key={d.serial} className="text-warn">
            <span className="font-mono">{d.model}</span> is {trouble.name}
            {trouble.fix && <span className="text-muted"> — {trouble.fix}</span>}
            {d.hint && <p className="mt-0.5 text-[11px] text-muted">{d.hint}</p>}
          </li>
        );
      })}
    </ul>
  );
}

/** ADB ships with the app, so it is deliberately absent from this list. */
function Prerequisites() {
  return (
    <div className="flex flex-col items-center gap-1.5 text-xs text-faint">
      <p>• Android device connected via USB</p>
      <p>• USB Debugging enabled in Developer Options</p>
    </div>
  );
}
