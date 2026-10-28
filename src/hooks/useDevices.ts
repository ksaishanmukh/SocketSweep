import { useEffect, useState } from "react";
import { commands, errorMessage, type Device } from "../api";

/** Fast enough that plugging in feels instant, slow enough to be free. */
const DEVICE_POLL_MS = 2000;

/**
 * The attached devices, polled while `paused` is false. `devices` is null
 * until the first answer; `problem` says why they could not be listed, e.g.
 * adb would not start.
 */
export function useDevices(paused: boolean) {
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  // Poll while idle so plugging a phone in is noticed without a manual refresh.
  // The cancelled flag matters: a poll in flight when this screen unmounts (the
  // usual case, since connecting navigates away) must not set state afterwards.
  useEffect(() => {
    if (paused) return;
    let cancelled = false;

    const poll = async () => {
      try {
        const list = await commands.listDevices();
        if (!cancelled) {
          setDevices(list);
          setProblem(null);
        }
      } catch (err) {
        // Nothing can be listed, and the person needs to know why: the host
        // starts adb on first use, so this is where a failure to start shows.
        if (!cancelled) {
          setDevices([]);
          setProblem(errorMessage(err));
        }
      }
    };

    void poll();
    const timer = setInterval(poll, DEVICE_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [paused]);

  return { devices, problem };
}
