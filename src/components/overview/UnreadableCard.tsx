import type { UnreadableFolder } from "../../api";
import { plural } from "../../lib/format";
import { nodeKey } from "../../lib/size";
import { Card } from "../common/Card";

export const UNREADABLE_CARD_ID = "unreadable";

/** Folders the phone would not let the scan read, so their sizes are missing from every total. */
export function UnreadableCard({ folders }: { folders: UnreadableFolder[] }) {
  return (
    <Card
      id={UNREADABLE_CARD_ID}
      title={<span className="text-warn">{plural(folders.length, "folder")} couldn’t be read</span>}
    >
      <p className="text-[13px] text-muted mb-2">
        Android keeps these system apps’ folders private, even over USB debugging. Their sizes
        aren’t in the totals.
      </p>
      <ul className="max-h-48 overflow-y-auto">
        {folders.map((f) => (
          <li key={nodeKey(f.node)} className="font-mono text-[12px] break-all py-0.5">
            {f.path}
          </li>
        ))}
      </ul>
    </Card>
  );
}
