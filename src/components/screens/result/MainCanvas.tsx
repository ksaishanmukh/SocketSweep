import { useMemo } from "react";
import type { Row, View } from "../../../api";
import type { ScanSession } from "../../../hooks/useScanSession";
import { sizeOf } from "../../../lib/size";
import { toItems } from "../../../lib/treemapItems";
import { AppBreakdown } from "../../AppBreakdown";
import { FileList } from "../../FileList";
import { Treemap } from "../../Treemap";
import { TypeBreakdown } from "../../TypeBreakdown";
import { copyOptions } from "./copyOptions";

/** The analysis the user picked, filling most of the screen. */
export function MainCanvas({
  session,
  view,
  running,
  onDelete,
}: {
  session: ScanSession;
  view: View;
  running: boolean;
  onDelete: (row: Row) => void;
}) {
  const { treemap, mode, metric, largest, types, apps, open } = session;
  const { items, counting } = useMemo(() => toItems(treemap, metric), [treemap, metric]);

  return (
    <div className="flex-1 min-h-0 glass-card rounded-xl overflow-hidden">
      {mode === "treemap" && (
        <Treemap
          items={items}
          counting={counting}
          emptyMessage={
            running && view.measure !== "final" ? "Measuring this folder…" : "Nothing to show here."
          }
          onOpen={(id, isDir) => {
            if (isDir && treemap) void open({ session: treemap.node.session, id });
          }}
        />
      )}

      {/* Biggest files anywhere on the device, regardless of depth. */}
      {mode === "largest" && (
        <FileList
          rows={largest}
          totalSize={largest[0] ? sizeOf(largest[0].size, metric) : 0}
          metric={metric}
          onOpen={(row) => void open(row.node)}
          onDelete={onDelete}
          {...copyOptions(session)}
          onReveal={(row) => void session.reveal(row)}
          emptyMessage={running ? "Looking for files…" : "No files found."}
        />
      )}

      {mode === "types" && <TypeBreakdown groups={types} metric={metric} />}

      {mode === "apps" && (
        <AppBreakdown
          apps={apps}
          metric={metric}
          onOpen={(node) => {
            // The app's folder is shown in the treemap, not in this list.
            session.setMode("treemap");
            void open(node);
          }}
        />
      )}
    </div>
  );
}
