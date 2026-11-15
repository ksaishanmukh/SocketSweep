import { useContext, useEffect, useSyncExternalStore } from "react";
import { appHue, appLetter, type AppLike } from "../../lib/appIdentity";
import { IconStoreContext } from "./iconContext";

/** The app's launcher icon once the phone has sent it, fetched the first time it is shown. */
function useAppIcon(app: AppLike): string | null {
  const store = useContext(IconStoreContext);
  const version = useSyncExternalStore(store.subscribe, store.version);
  useEffect(() => {
    if (app.hasIcon) store.want(app.package);
  }, [app.hasIcon, app.package, store, version]);
  return app.hasIcon ? store.get(app.package) : null;
}

/**
 * The real icon, or until there is one a lettered tile in a colour of its own
 * from the package name. Decorative: the name always stands beside it.
 */
export function AppIcon({ app, size = 32 }: { app: AppLike; size?: number }) {
  const src = useAppIcon(app);
  const box = { width: size, height: size, borderRadius: Math.round(size * 0.24) };
  if (src) {
    return <img src={src} alt="" aria-hidden style={box} className="shrink-0 object-cover" />;
  }
  return (
    <span
      aria-hidden
      className="shrink-0 inline-flex items-center justify-center font-semibold text-white select-none"
      style={{
        ...box,
        background: `hsl(${appHue(app.package)} 42% 40%)`,
        fontSize: Math.round(size * 0.46),
      }}
    >
      {appLetter(app)}
    </span>
  );
}
