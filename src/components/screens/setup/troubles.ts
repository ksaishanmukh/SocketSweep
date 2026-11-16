import type { DeviceState } from "../../../api";

/** How each non-usable state is named, and what to do about it, since the fix differs. */
const TROUBLE: Record<Exclude<DeviceState, "ready">, { name: string; fix?: string }> = {
  unauthorized: { name: "Waiting for permission" },
  offline: { name: "Offline", fix: "Unplug and reconnect the cable." },
  noPermission: { name: "Blocked", fix: "The system is blocking access to this device." },
  other: { name: "Not ready" },
};

export function troubleOf(state: DeviceState): { name: string; fix?: string } {
  return state === "ready" ? { name: "Not usable" } : TROUBLE[state];
}
