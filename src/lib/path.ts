/** Joins a directory and a name without doubling the separator. */
export function joinPath(dir: string, name: string): string {
  return dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`;
}

/** True for `path` itself and anything below it (not for look-alike siblings). */
export function isWithin(path: string, folder: string): boolean {
  return path === folder || path.startsWith(folder.endsWith("/") ? folder : folder + "/");
}
