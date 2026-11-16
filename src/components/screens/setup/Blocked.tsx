import type { Device } from "../../../api";
import { DeviceRow } from "./DeviceRow";
import { troubleOf } from "./troubles";

/** Attached but unusable: say which problem it is, and what to do about it. */
export function Blocked({ device }: { device: Device }) {
  const trouble = troubleOf(device.state);
  return (
    <>
      <DeviceRow model={device.model} tone="warn">
        <span className="text-warn">
          <span
            className="inline-block w-[7px] h-[7px] rounded-full bg-warn mr-1.5 animate-pulse"
            aria-hidden
          />
          {trouble.name}
        </span>
      </DeviceRow>
      {device.state === "unauthorized" ? (
        <p className="mt-4 text-[14px] leading-relaxed text-muted">
          Unlock your phone and tap <strong className="text-fg">Allow</strong> on the “Allow USB
          debugging?” prompt. Tick “Always allow from this computer” to skip this next time.
        </p>
      ) : (
        trouble.fix && <p className="mt-4 text-[14px] text-muted">{trouble.fix}</p>
      )}
      {device.hint && <p className="mt-2 text-[13px] text-muted">{device.hint}</p>}
      <button
        type="button"
        disabled
        className="mt-5 w-full h-[46px] rounded-xl bg-sunk text-faint font-semibold text-[15px]"
      >
        Scan this phone
      </button>
    </>
  );
}
