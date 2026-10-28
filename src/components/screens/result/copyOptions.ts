import type { Row } from "../../../api";
import type { ScanSession } from "../../../hooks/useScanSession";
import { folderName } from "../../../lib/path";

/** The copy-related props every `FileList` on the result screen takes. */
export function copyOptions(session: ScanSession) {
  return {
    onCopy: (row: Row) => void session.startCopy(row),
    copyDisabled: session.copy !== null,
    copyTarget: session.copyDestination ? folderName(session.copyDestination) : undefined,
  };
}
