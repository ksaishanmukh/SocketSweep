import { formatBytes, formatNumber } from "../../lib/format";
import type { Stats } from "../../lib/types";

/** Live totals from `scan-progress`, updated as the walk streams in. */
export function ScanningScreen({ stats, current }: { stats: Stats | null; current: string }) {
  return (
    <div className="flex flex-col items-center justify-center flex-1 gap-8 animate-fade-in-up">
      <div className="relative w-32 h-32">
        <div className="absolute inset-0 rounded-full border-2 border-accent-500/20" />
        <svg
          className="absolute inset-0 animate-spin"
          style={{ animationDuration: "2s" }}
          viewBox="0 0 128 128"
          aria-hidden
        >
          <circle
            cx="64"
            cy="64"
            r="62"
            fill="none"
            stroke="url(#scanGrad)"
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeDasharray="120 280"
          />
          <defs>
            <linearGradient id="scanGrad" x1="0%" y1="0%" x2="100%" y2="0%">
              <stop offset="0%" stopColor="oklch(0.60 0.18 180)" />
              <stop offset="100%" stopColor="oklch(0.60 0.18 180 / 0)" />
            </linearGradient>
          </defs>
        </svg>
        <div className="absolute inset-0 flex items-center justify-center">
          <div className="w-16 h-16 rounded-xl bg-accent-500/10 border border-accent-500/20 flex items-center justify-center scan-shimmer">
            <span className="text-accent-400 text-xl" aria-hidden>
              ⚡
            </span>
          </div>
        </div>
      </div>

      <div className="text-center" role="status" aria-live="polite">
        <h2 className="text-lg font-semibold text-zinc-200">Scanning storage</h2>
        <p className="mt-2 text-3xl font-mono font-bold text-accent-400 tabular-nums">
          {stats ? formatBytes(stats.size) : "—"}
        </p>
        <p className="mt-1 text-sm text-zinc-500 font-mono tabular-nums">
          {stats ? `${formatNumber(stats.files)} files · ${formatNumber(stats.dirs)} folders` : " "}
        </p>
        <p className="mt-3 text-xs text-zinc-600 font-mono truncate max-w-md mx-auto">{current}</p>
      </div>
    </div>
  );
}
