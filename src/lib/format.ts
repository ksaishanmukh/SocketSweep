const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"] as const;

/**
 * Human-readable byte count.
 *
 * Guards three cases the original inline version got wrong, all of which
 * rendered the literal string "undefined" into the UI:
 *   - sizes at or above 1 PB indexed past the end of the unit table
 *   - negative sizes (reachable if a delete over-subtracts) produced NaN
 *   - values between 0 and 1 produced a negative exponent
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
