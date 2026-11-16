import type { Device } from "../../../api";
import { useDevices } from "../../../hooks/useDevices";
import { BrandIcon } from "../../common/BrandIcon";
import { Blocked } from "./Blocked";
import { ConnectingSteps } from "./ConnectingSteps";
import { DeviceRow, UsbLine } from "./DeviceRow";
import { NoDeviceSteps } from "./NoDeviceSteps";

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
  const blocked = devices?.find((d) => !d.usable);

  return (
    <div className="flex flex-col items-center justify-center flex-1 px-6 rise-in">
      <div className="w-full max-w-[420px] flex flex-col items-center">
        <BrandIcon size={72} />
        <h1 className="mt-5 text-[28px] font-bold tracking-tight text-fg">SocketSweep</h1>
        <p className="mt-1.5 mb-7 text-[15px] text-muted text-center">
          See what is filling your phone's storage.
        </p>

        <div className="w-full bg-panel border border-line rounded-2xl p-[22px]">
          {loading ? (
            <Connecting device={usable[0]} status={status} />
          ) : (
            <Idle usable={usable} blocked={blocked} onConnect={onConnect} />
          )}
        </div>

        {!loading && problem && (
          <p role="alert" className="mt-4 text-center text-xs text-warn">
            {problem}
          </p>
        )}
        <p className="mt-5 text-xs text-faint text-center">
          Works over USB. Nothing is installed on your phone. Android 11 or newer.
        </p>
      </div>
    </div>
  );
}

function Connecting({ device, status }: { device?: Device; status: string }) {
  return (
    <>
      <DeviceRow model={device?.model ?? "Your phone"}>Getting ready to scan…</DeviceRow>
      <ConnectingSteps status={status} />
    </>
  );
}

function Idle({
  usable,
  blocked,
  onConnect,
}: {
  usable: Device[];
  blocked?: Device;
  onConnect: (serial?: string) => void;
}) {
  if (usable.length > 1) return <Picker devices={usable} onConnect={onConnect} />;
  if (usable.length === 1) {
    const [d] = usable;
    return (
      <>
        <DeviceRow model={d.model}>
          <UsbLine serial={d.serial} />
        </DeviceRow>
        <button
          type="button"
          onClick={() => onConnect()}
          className="mt-5 w-full h-[46px] rounded-xl bg-accent text-on-accent font-semibold text-[15px] hover:opacity-90"
        >
          Scan this phone
        </button>
      </>
    );
  }
  return blocked ? <Blocked device={blocked} /> : <NoDeviceSteps />;
}

function Picker({
  devices,
  onConnect,
}: {
  devices: Device[];
  onConnect: (serial: string) => void;
}) {
  return (
    <div role="group" aria-label="Choose a phone">
      <h2 className="text-[15px] font-semibold text-fg">Choose a phone</h2>
      <ul className="mt-3 flex flex-col gap-2">
        {devices.map((d) => (
          <li key={d.serial}>
            <button
              type="button"
              onClick={() => onConnect(d.serial)}
              className="w-full p-3 rounded-xl border border-line hover:border-accent transition-colors"
            >
              <DeviceRow model={d.model}>
                <UsbLine serial={d.serial} />
              </DeviceRow>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
