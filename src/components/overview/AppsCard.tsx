import type { AppUsage, NodeRef, SizeMetric } from "../../api";
import { appName } from "../../lib/appIdentity";
import { formatBytes } from "../../lib/format";
import { sizeOf } from "../../lib/size";
import { AppIcon } from "../common/AppIcon";
import { Card } from "../common/Card";

/**
 * Storage owned by each app, from its folders under `Android/data`, `media`
 * and `obb`. Bars are relative to the largest app, not the total: the point
 * is comparing apps, and a leading 40 GB game would flatten the rest.
 */
export function AppsCard({
  apps,
  metric,
  onOpen,
}: {
  apps: AppUsage[];
  metric: SizeMetric;
  onOpen: (node: NodeRef) => void;
}) {
  const largest = apps.length > 0 ? sizeOf(apps[0].size, metric) : 0;
  return (
    <Card id="space-by-app" title="Space by app">
      {apps.length === 0 ? (
        <p className="text-[13px] text-muted">
          No app folders found under Android/data, media or obb.
        </p>
      ) : (
        <ul className="-mx-2">
          {apps.map((app) => (
            <li key={app.package}>
              <AppEntry app={app} metric={metric} largest={largest} onOpen={onOpen} />
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

function AppEntry({
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
  const bytes = sizeOf(app.size, metric);
  const share = largest > 0 ? (bytes / largest) * 100 : 0;
  return (
    <button
      type="button"
      onClick={() => onOpen(app.node)}
      title={`Open the folder of ${appName(app)}`}
      className="w-full flex items-center gap-3 px-2 py-1.5 rounded-lg hover:bg-sunk text-left"
    >
      <AppIcon app={app} size={32} />
      <span className="min-w-0 flex-1">
        <span className="flex items-baseline gap-2">
          <span className="truncate text-[14px] font-semibold flex-1">{appName(app)}</span>
          <span className="num text-[13px] shrink-0">{formatBytes(bytes)}</span>
        </span>
        {app.label && (
          <span className="block truncate font-mono text-[11px] text-muted">{app.package}</span>
        )}
        <span className="block mt-1 h-1 rounded-full bg-sunk overflow-hidden" aria-hidden>
          <span
            className="block h-full rounded-full bg-accent"
            style={{ width: `${Math.max(1, share)}%` }}
          />
        </span>
      </span>
    </button>
  );
}
