import { openUrl } from "@tauri-apps/plugin-opener";
import type { Release } from "../lib/update";

/** A calm one-line banner: a newer release exists. Dismissing it is remembered for that version. */
export function UpdateNotice({ release, onDismiss }: { release: Release; onDismiss: () => void }) {
  return (
    <div
      role="status"
      aria-live="polite"
      className="shrink-0 flex items-center justify-center gap-3 px-4 py-1.5 bg-panel border-b border-line text-[13px] text-muted"
    >
      <span>SocketSweep {release.version} is available.</span>
      <a
        href={release.url}
        onClick={(event) => {
          event.preventDefault();
          void openUrl(release.url).catch(() => {});
        }}
        className="text-accent font-medium hover:underline"
      >
        Download
      </a>
      <button
        type="button"
        onClick={onDismiss}
        aria-label="Dismiss"
        className="w-6 h-6 rounded-md text-muted hover:text-fg hover:bg-sunk"
      >
        ✕
      </button>
    </div>
  );
}
