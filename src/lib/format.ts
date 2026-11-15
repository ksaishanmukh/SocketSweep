const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"] as const;

/**
 * Human-readable byte count.
 *
 * Guards three cases that would otherwise render "undefined" or "NaN":
 *   - sizes at or above 1 PB, which index past the end of the unit table
 *   - negative sizes, reachable if a delete over-subtracts
 *   - values between 0 and 1, which give a negative exponent
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes)) return "—";
  if (bytes < 0) return `-${formatBytes(-bytes)}`;
  if (bytes < 1) return "0 B";

  const exp = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), UNITS.length - 1);
  const value = bytes / 1024 ** exp;
  return `${value.toFixed(exp === 0 ? 0 : 1)} ${UNITS[exp]}`;
}

export function formatNumber(n: number): string {
  if (!Number.isFinite(n)) return "—";
  return n.toLocaleString("en-US");
}

const DATE = new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "short", year: "numeric" });

/** A change time in Unix seconds as a short date, or a dash while it is unknown. */
export function formatDate(unixSeconds: number | null): string {
  if (unixSeconds === null || !Number.isFinite(unixSeconds)) return "—";
  return DATE.format(new Date(unixSeconds * 1000));
}

/** A signed size change: "+1.2 GB", "−340.0 MB". */
export function formatDelta(bytes: number): string {
  if (bytes === 0) return "no change";
  return `${bytes > 0 ? "+" : "−"}${formatBytes(Math.abs(bytes))}`;
}

/** What kind of file a name is, by its extension: "ZIP file", or just "File". */
export function fileKind(name: string): string {
  const dot = name.lastIndexOf(".");
  // A leading dot is a hidden file's name, not an extension.
  if (dot <= 0 || dot === name.length - 1) return "File";
  const ext = name.slice(dot + 1);
  return ext.length > 8 ? "File" : `${ext.toUpperCase()} file`;
}

/** "1 file", "1,263 files". */
export function plural(n: number, one: string, many = `${one}s`): string {
  return `${formatNumber(n)} ${n === 1 ? one : many}`;
}
