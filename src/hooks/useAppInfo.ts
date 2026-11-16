import { useCallback, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { checkForUpdate, dismiss, type Release } from "../lib/update";

/**
 * The app's own version and, once a launch, whether a newer release exists.
 * Outside the Tauri shell (tests, a plain browser) there is no version and
 * so no check.
 */
export function useAppInfo() {
  const [version, setVersion] = useState<string | null>(null);
  const [update, setUpdate] = useState<Release | null>(null);

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const current = await getVersion();
        if (!live) return;
        setVersion(current);
        const release = await checkForUpdate(current);
        if (live) setUpdate(release);
      } catch {
        // No version to show, nothing to check.
      }
    })();
    return () => {
      live = false;
    };
  }, []);

  const dismissUpdate = useCallback(() => {
    if (update) dismiss(update.version);
    setUpdate(null);
  }, [update]);

  return { version, update, dismissUpdate };
}
