import type { ScanSession } from "../../hooks/useScanSession";
import { AppsCard } from "./AppsCard";
import { BiggestFiles } from "./BiggestFiles";
import { CategoryBar } from "./CategoryBar";
import { LeftoversCard } from "./LeftoversCard";
import { SinceLastScanCard } from "./SinceLastScanCard";
import { UnreadableCard } from "./UnreadableCard";

/** The whole phone at a glance: what type of thing, which files, which apps, what is left over. */
export function OverviewScreen({ session }: { session: ScanSession }) {
  const { overview, metric, selection, actions, scanInfo } = session;
  const running = scanInfo?.running ?? false;

  if (!overview) {
    return <p className="p-8 text-center text-[14px] text-muted">Putting the overview together…</p>;
  }

  return (
    <div className="max-w-[1240px] mx-auto p-6 grid grid-cols-12 gap-4 items-start">
      <CategoryBar
        categories={overview.categories}
        metric={metric}
        onPick={(c) => void actions.showCategory(c)}
      />
      <BiggestFiles
        rows={overview.biggestFiles}
        metric={metric}
        selection={selection}
        running={running}
        onReveal={(row) => void actions.reveal(row)}
      />
      <div className="col-span-12 lg:col-span-5 flex flex-col gap-4">
        <AppsCard
          apps={overview.apps}
          metric={metric}
          onOpen={(node) => void actions.explore(node)}
        />
        <LeftoversCard
          leftovers={overview.leftovers}
          status={overview.appsStatus}
          metric={metric}
          selection={selection}
        />
        <SinceLastScanCard
          since={overview.sinceLastScan}
          model={session.model}
          metric={metric}
          running={running}
        />
        {overview.unreadable.length > 0 && <UnreadableCard folders={overview.unreadable} />}
      </div>
    </div>
  );
}
