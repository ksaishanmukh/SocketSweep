/** What every app-tagged thing shares, rows and the overview's app list alike. */
export interface AppLike {
  package: string;
  label: string | null;
  hasIcon: boolean;
}

/** The launcher's name once the phone has reported it; until then the package, never a guess. */
export function appName(app: AppLike): string {
  return app.label ?? app.package;
}

/**
 * The letter for an app without an icon: the label's first, else the first
 * of the package's last segment (`com.whatsapp` → W), which is usually the
 * app's own name.
 */
export function appLetter(app: AppLike): string {
  const source = app.label ?? app.package.split(".").pop() ?? app.package;
  const letter = source.match(/[\p{L}\p{N}]/u)?.[0] ?? "?";
  return letter.toUpperCase();
}

/** A hue from the package name, so the same app always gets the same tile. */
export function appHue(pkg: string): number {
  let hash = 0;
  for (let i = 0; i < pkg.length; i++) hash = (hash * 31 + pkg.charCodeAt(i)) >>> 0;
  return hash % 360;
}
