import type { AppUsage, NodeRef, SizeMetric } from "../api";
import { formatBytes, formatNumber } from "../lib/format";
import { sizeOf } from "../lib/size";

/**
 * Storage attributed to the app that owns it.
 *
 * Android puts per-app files under `Android/{data,obb,media}/<package>`, so
 * ownership follows from location. This works where file types do not: most
 * space is game assets with unrecognised extensions.
 */
export function AppBreakdown({
  apps,
  metric,
  onOpen,
}: {
  apps: AppUsage[];
  metric: SizeMetric;
  onOpen: (node: NodeRef) => void;
}) {
  if (apps.length === 0) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-zinc-600 px-6 text-center">
        No per-app storage found. This device has no Android/data or Android/obb folders, or they
        are not readable.
      </div>
    );
  }

  const largest = sizeOf(apps[0].size, metric);

  return (
    <div className="h-full overflow-y-auto p-5">
      <p className="text-[11px] text-zinc-600 mb-4">
        Space under <span className="font-mono">Android/data</span>,{" "}
        <span className="font-mono">obb</span> and <span className="font-mono">media</span>, grouped
        by the app that owns it.
      </p>

      <ul className="flex flex-col gap-2.5">
        {apps.map((app) => (
          <AppRow key={app.package} app={app} metric={metric} largest={largest} onOpen={onOpen} />
        ))}
      </ul>
    </div>
  );
}

function AppRow({
  app,
  metric,
  largest,
  onOpen,
}: {
  app: AppUsage;
  metric: SizeMetric;
  largest: number;
  onOpen: (node: NodeRef) => void;
}) {
  // Relative to the largest app rather than the total: the point is
  // comparing apps to each other, and a leading 45GB game would
  // otherwise flatten everything below it into invisible slivers.
  const size = sizeOf(app.size, metric);
  const share = largest > 0 ? (size / largest) * 100 : 0;

  return (
    <li>
      <button
        onClick={() => onOpen(app.node)}
        className="w-full text-left group rounded-md px-2 py-1.5 hover:bg-white/5
                  focus-visible:outline-2 focus-visible:outline-accent-500"
        title={`Go to ${app.package}`}
      >
        <div className="flex items-baseline gap-3">
          {/*
            Shown whole, in mono, as the identifier it is. Emphasising the last
            segment would imply an app name, but Android gives no label.
          */}
          <span className="flex-1 min-w-0 truncate text-sm font-mono text-zinc-300">
            {app.package}
          </span>
          <span className="text-xs font-mono text-zinc-300 tabular-nums shrink-0">
            {formatBytes(size)}
          </span>
          <span className="text-[11px] text-zinc-600 tabular-nums w-24 text-right shrink-0">
            {formatNumber(app.files)} files
          </span>
        </div>
        <div className="mt-1 h-1.5 rounded-full bg-zinc-800 overflow-hidden">
          <div
            className="h-full rounded-full bg-accent-500 group-hover:bg-accent-400 transition-colors"
            style={{ width: `${Math.max(0.5, share)}%` }}
          />
        </div>
      </button>
    </li>
  );
}
