/**
 * Share of the folder currently on screen, in a single hue.
 *
 * The version this replaces measured each row against its immediate parent and
 * mapped the result to red / amber / teal, which made the root row permanently
 * red and left "red" meaning nothing at all. Red is now reserved for
 * destructive actions.
 */
export function SizeBar({ ratio }: { ratio: number }) {
  const pct = Math.max(0.5, Math.min(1, ratio) * 100);
  return (
    <div className="w-24 h-1.5 rounded-full bg-zinc-800 overflow-hidden flex-shrink-0" aria-hidden>
      <div
        className="h-full rounded-full size-bar-inner bg-accent-500"
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}
