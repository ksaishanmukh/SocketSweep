/**
 * The new-release notice: whether GitHub's latest release is newer than this
 * build, asked at most once a day, and which one the person already waved off.
 * Everything here fails quietly; a missing notice is never worth an error.
 */

export const RELEASE_URL = "https://api.github.com/repos/ksaishanmukh/SocketSweep/releases/latest";
/** Only links under here are offered, whatever the response says. */
const RELEASE_PAGE_PREFIX = "https://github.com/ksaishanmukh/SocketSweep/";
export const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

const CHECKED_KEY = "socketsweep.update.checkedAt";
const DISMISSED_KEY = "socketsweep.update.dismissed";

export interface Release {
  version: string;
  url: string;
}

/** "v2.1.0" to [2, 1, 0]; null for anything but plain `major.minor.patch` (prereleases included). */
export function parseVersion(text: string): [number, number, number] | null {
  const match = /^v?(\d+)\.(\d+)\.(\d+)$/.exec(text.trim());
  return match ? [Number(match[1]), Number(match[2]), Number(match[3])] : null;
}

/** True when `latest` is a plain version above `current`; an unreadable one is never newer. */
export function isNewer(latest: string, current: string): boolean {
  const a = parseVersion(latest);
  const b = parseVersion(current);
  if (!a || !b) return false;
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] > b[i];
  return false;
}

/** The release in a GitHub "latest" response if it is newer than `current`. */
export function newerRelease(body: unknown, current: string): Release | null {
  if (typeof body !== "object" || body === null) return null;
  const { tag_name, html_url, prerelease, draft } = body as Record<string, unknown>;
  if (typeof tag_name !== "string" || typeof html_url !== "string") return null;
  if (prerelease === true || draft === true) return null;
  if (!html_url.startsWith(RELEASE_PAGE_PREFIX) || !isNewer(tag_name, current)) return null;
  return { version: tag_name.trim().replace(/^v/, ""), url: html_url };
}

function read(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Without storage the check just runs again next launch.
  }
}

export function isDismissed(version: string): boolean {
  return read(DISMISSED_KEY) === version;
}

export function dismiss(version: string) {
  write(DISMISSED_KEY, version);
}

/**
 * Asks GitHub for the latest release, unless that was done in the last day.
 * Resolves to the release when it is newer and not dismissed; null otherwise,
 * including offline, rate limited or malformed.
 */
export async function checkForUpdate(
  current: string,
  now: number = Date.now(),
  fetcher: typeof fetch = fetch,
): Promise<Release | null> {
  const last = Number(read(CHECKED_KEY));
  if (Number.isFinite(last) && last > 0 && now - last < CHECK_INTERVAL_MS) return null;
  try {
    // Stamped before the answer so a failing network is not retried every launch.
    write(CHECKED_KEY, String(now));
    const response = await fetcher(RELEASE_URL, {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!response.ok) return null;
    const release = newerRelease(await response.json(), current);
    return release && !isDismissed(release.version) ? release : null;
  } catch {
    return null;
  }
}
