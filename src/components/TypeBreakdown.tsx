import { formatBytes, formatNumber } from "../lib/format";
import type { TypeGroup } from "../lib/types";

/**
 * Storage by kind of file.
 *
 * "24 GB of video" is a sentence someone can act on. No amount of
 * folder-by-folder browsing produces it, which is why the old UI could not
 * answer the question at all.
 */

/** One hue per category, stable regardless of ordering. */
const HUES: Record<string, string> = {
  Photos: "#0284c7",
  Video: "#7c3aed",
  Audio: "#c026d3",
  Apps: "#0d9488",
  Documents: "#ca8a04",
  Archives: "#ea580c",
  Other: "#52525b",
};

export function TypeBreakdown({ groups }: { groups: TypeGroup[] }) {
  const total = groups.reduce((sum, g) => sum + g.size, 0);

  if (groups.length === 0) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-zinc-600">
        Nothing scanned yet.
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto p-5">
      {/* A single stacked bar first, so the proportions read at a glance. */}
      <div
        className="flex h-3 rounded-full overflow-hidden mb-6"
        role="img"
        aria-label="Share of storage by file type"
      >
        {groups.map((g) => (
          <div
            key={g.label}
            style={{
              width: `${total > 0 ? (g.size / total) * 100 : 0}%`,
              background: HUES[g.label] ?? HUES.Other,
            }}
            title={`${g.label} — ${formatBytes(g.size)}`}
          />
        ))}
      </div>

      <ul className="flex flex-col gap-3">
        {groups.map((g) => {
          const share = total > 0 ? (g.size / total) * 100 : 0;
          return (
            <li key={g.label} className="flex items-center gap-3">
              <span
                className="w-2.5 h-2.5 rounded-sm shrink-0"
                style={{ background: HUES[g.label] ?? HUES.Other }}
                aria-hidden
              />
              <span className="text-sm text-zinc-300 w-24 shrink-0">{g.label}</span>

              <div className="flex-1 h-1.5 rounded-full bg-zinc-800 overflow-hidden">
                <div
                  className="h-full rounded-full"
                  style={{
                    width: `${Math.max(0.5, share)}%`,
                    background: HUES[g.label] ?? HUES.Other,
                  }}
                />
              </div>

              <span className="text-xs font-mono text-zinc-400 w-20 text-right tabular-nums shrink-0">
                {formatBytes(g.size)}
              </span>
              <span className="text-xs text-zinc-600 w-14 text-right tabular-nums shrink-0">
                {share.toFixed(1)}%
              </span>
              <span className="text-xs text-zinc-600 w-24 text-right tabular-nums shrink-0">
                {formatNumber(g.files)} files
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
