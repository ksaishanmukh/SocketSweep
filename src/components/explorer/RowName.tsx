import type { Row } from "../../api";
import { appName } from "../../lib/appIdentity";
import { categoryColor } from "../../lib/categories";
import { AppIcon } from "../common/AppIcon";
import { IconFile, IconFolder } from "../fileIcons";

/**
 * The glyph and name. An app's folder shows the app instead of a folder, with
 * its package underneath once the launcher name is known; a search hit shows
 * the folder it lives in, which goes there.
 */
export function RowName({
  row,
  onOpen,
  onReveal,
}: {
  row: Row;
  onOpen: (row: Row) => void;
  onReveal: (row: Row) => void;
}) {
  const isDir = row.kind === "dir";
  const title = row.app ? appName(row.app) : row.name;
  return (
    <span role="cell" className="flex items-center gap-2.5 min-w-0">
      {row.app ? (
        <AppIcon app={row.app} size={24} />
      ) : (
        <span className="shrink-0" style={{ color: categoryColor(row.category) }}>
          {isDir ? <IconFolder /> : <IconFile />}
        </span>
      )}
      <span className={`min-w-0 flex-1 ${row.staged ? "line-through" : ""}`}>
        {isDir ? (
          <button
            type="button"
            onClick={() => onOpen(row)}
            title={row.name}
            className="block max-w-full truncate text-left text-[14px] font-medium hover:underline"
          >
            {title}
          </button>
        ) : (
          <span className="block truncate text-[14px]" title={row.name}>
            {title}
          </span>
        )}
        <Subline row={row} onReveal={onReveal} />
      </span>
      {row.staged && <PendingMarker />}
      {row.unreadable && (
        <span
          className="text-[12px] text-warn shrink-0"
          role="img"
          title="Some folders in here couldn’t be read, so this size is low"
          aria-label="Some folders could not be read"
        >
          ⚠
        </span>
      )}
    </span>
  );
}

/** Says why a name is struck through, in words as well as the strike. */
export function PendingMarker() {
  return (
    <span className="shrink-0 rounded border border-line px-1.5 text-[11px] text-muted">
      pending delete
    </span>
  );
}

/** A search hit's folder, which goes there, or an app folder's package once its name is known. */
function Subline({ row, onReveal }: { row: Row; onReveal: (row: Row) => void }) {
  if (row.parent) {
    return (
      <button
        type="button"
        onClick={() => onReveal(row)}
        title={`Go to ${row.parent}`}
        className="block max-w-full truncate text-left font-mono text-[11px] text-muted hover:text-accent hover:underline"
      >
        {row.parent}
      </button>
    );
  }
  if (!row.app?.label) return null;
  return <span className="block truncate font-mono text-[11px] text-muted">{row.app.package}</span>;
}
