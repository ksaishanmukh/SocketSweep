import { createContext } from "react";
import type { IconStore } from "../../lib/iconStore";

const NO_ICONS: IconStore = {
  get: () => null,
  want: () => {},
  retryMisses: () => {},
  clear: () => {},
  subscribe: () => () => {},
  version: () => 0,
};

/** The session's icon cache. Without a provider nothing is fetched and every app gets its letter. */
export const IconStoreContext = createContext<IconStore>(NO_ICONS);
