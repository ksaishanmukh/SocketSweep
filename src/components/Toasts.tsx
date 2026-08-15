import type { Toast } from "../hooks/useToasts";

const STYLES: Record<Toast["type"], string> = {
  error: "bg-red-950/80 border-red-800/50 text-red-200",
  success: "bg-emerald-950/80 border-emerald-800/50 text-emerald-200",
  info: "bg-zinc-800/80 border-zinc-700/50 text-zinc-200",
};

const GLYPHS: Record<Toast["type"], string> = {
  error: "✕",
  success: "✓",
  info: "ℹ",
};

export function Toasts({
  toasts,
  onDismiss,
}: {
  toasts: Toast[];
  onDismiss: (id: number) => void;
}) {
  return (
    // Assertive for errors would interrupt; polite lets a screen reader finish
    // its sentence first. Toasts are never the only channel for a message.
    <div
      className="fixed top-4 right-4 z-50 flex flex-col gap-2 max-w-sm"
      role="status"
      aria-live="polite"
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`${t.exiting ? "animate-toast-out" : "animate-toast-in"}
            flex items-start gap-3 px-4 py-3 rounded-lg border shadow-xl backdrop-blur-md
            ${STYLES[t.type]}`}
        >
          <span className="mt-0.5 text-base" aria-hidden>
            {GLYPHS[t.type]}
          </span>
          <p className="text-sm leading-relaxed flex-1">{t.message}</p>
          <button
            onClick={() => onDismiss(t.id)}
            className="text-current/50 hover:text-current rounded focus-visible:outline-2 focus-visible:outline-current"
            aria-label="Dismiss"
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
