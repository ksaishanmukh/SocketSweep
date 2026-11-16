/** The app mark: a treemap, so the icon says what the app shows. Same shapes as src-tauri/icons/icon.svg. */
export function BrandIcon({ size = 20 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden>
      <rect width="64" height="64" rx="14" fill="#2b55c9" />
      <rect x="9" y="9" width="25" height="46" rx="4" fill="#dbe5ff" />
      <rect x="37" y="9" width="18" height="21" rx="4" fill="#ffb46b" />
      <rect x="37" y="33" width="8" height="22" rx="3" fill="#7fe0d3" />
      <rect x="48" y="33" width="7" height="22" rx="3" fill="#a9c1ff" />
    </svg>
  );
}
