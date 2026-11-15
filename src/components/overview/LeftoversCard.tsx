import type { AppsStatus, Leftovers, SizeMetric } from "../../api";
import type { Selection } from "../../hooks/useSelection";
import { formatBytes, plural } from "../../lib/format";
import { targetOfLeftover } from "../../lib/selection";
import { sizeOf } from "../../lib/size";
import { Card } from "../common/Card";
import { Checkbox } from "../common/Checkbox";

/**
 * Folders of apps that are no longer installed. Only the phone can say what
 * is installed, so until it has answered there is nothing to list.
 */
export function LeftoversCard({
  leftovers,
  status,
  metric,
  selection,
}: {
  leftovers: Leftovers;
  status: AppsStatus;
  metric: SizeMetric;
  selection: Selection;
}) {
  const found = status === "ready" && leftovers.apps.length > 0;
  return (
    <Card
      id="leftovers"
      title="Leftovers from uninstalled apps"
      aside={
        found && (
          <span className="num text-[13px] text-muted">
            {formatBytes(sizeOf(leftovers.total, metric))}
          </span>
        )
      }
    >
      <LeftoversBody leftovers={leftovers} status={status} metric={metric} selection={selection} />
    </Card>
  );
}

function LeftoversBody({
  leftovers,
  status,
  metric,
  selection,
}: {
  leftovers: Leftovers;
  status: AppsStatus;
  metric: SizeMetric;
  selection: Selection;
}) {
  if (status === "pending") return <p className="text-[13px] text-muted">Looking up apps…</p>;
  if (status === "unavailable") {
    return (
      <p className="text-[13px] text-muted">
        The phone couldn’t say which apps are installed, so leftovers can’t be found this time.
      </p>
    );
  }
  if (leftovers.apps.length === 0) {
    return (
      <p className="text-[13px] text-muted">
        None on this phone. Every app folder under Android/ belongs to an installed app.
      </p>
    );
  }
  return (
    <>
      <p className="text-[13px] text-muted mb-2">
        These apps are gone but left their folders behind. Usually safe to delete.
      </p>
      <ul className="-mx-2">
        {leftovers.apps.map((l) => {
          const target = targetOfLeftover(l);
          return (
            <li
              key={target.key}
              className="flex items-center gap-3 px-2 py-1.5 rounded-lg hover:bg-sunk min-h-11"
            >
              <Checkbox
                checked={selection.has(target.key)}
                label={`Select the leftovers of ${l.package}`}
                onChange={() => selection.toggle(target)}
              />
              <span className="min-w-0 flex-1">
                <span className="block truncate font-mono text-[13px]">{l.package}</span>
                <span className="block text-[12px] text-muted">
                  {plural(l.folders.length, "folder")}
                </span>
              </span>
              <span className="num text-[13px]">{formatBytes(sizeOf(l.size, metric))}</span>
            </li>
          );
        })}
      </ul>
    </>
  );
}
