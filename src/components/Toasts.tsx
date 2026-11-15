import type { Toast } from "../hooks/useToasts";

/** The glyph carries the colour; the message stays in normal text colour for contrast. */
const GLYPHS: Record<Toast["type"], { glyph: string; className: string }> = {
  error: { glyph: "✕", className: "text-danger" },
  success: { glyph: "✓", className: "text-ok" },
  info: { glyph: "ℹ", className: "text-accent" },
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
      className="fixed top-4 right-4 z-[110] flex flex-col gap-2 max-w-md"
      role="status"
      aria-live="polite"
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`card shadow-xl flex items-start gap-3 px-4 py-3 transition-opacity duration-300 ${
            t.exiting ? "opacity-0" : "rise-in"
          } ${t.type === "error" ? "border-danger" : ""}`}
        >
          <span className={`mt-0.5 text-base ${GLYPHS[t.type].className}`} aria-hidden>
            {GLYPHS[t.type].glyph}
          </span>
          <p className="text-[14px] leading-relaxed flex-1 break-words min-w-0">{t.message}</p>
          <button
            type="button"
            onClick={() => onDismiss(t.id)}
            className="w-8 h-8 -m-1 rounded-lg text-muted hover:text-fg hover:bg-sunk shrink-0"
            aria-label="Dismiss"
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
