/**
 * A stand-in for `src/api` in tests. Nothing here can reach a real host.
 *
 * Use it from a test file with:
 *
 *     vi.mock("../api", async (importOriginal) => ({
 *       ...(await importOriginal<typeof import("../api")>()),
 *       ...(await import("../test/api")).apiMock,
 *     }));
 *
 * which keeps the real `errorMessage` and friends and swaps `commands` and
 * `events`. Every command is a `vi.fn`; `emit` pushes an event to whoever
 * subscribed, as the host would.
 */
import { vi, type Mock } from "vitest";
import type {
  CommandError,
  ConnectionLost,
  Connected,
  CopyProgress,
  ErrorCode,
  NodeRef,
  Row,
  ScanProgress,
  ScanStarted,
  Size,
  Totals,
  TreemapNode,
  View,
  commands as realCommands,
} from "../api";

type Commands = typeof realCommands;
type MockedCommands = { [K in keyof Commands]: Mock<Commands[K]> };

const COMMAND_NAMES: (keyof Commands)[] = [
  "listDevices",
  "connect",
  "disconnect",
  "startScan",
  "cancelScan",
  "focus",
  "getView",
  "getBreadcrumbs",
  "getTreemap",
  "largestFiles",
  "typeBreakdown",
  "appBreakdown",
  "search",
  "delete",
  "copyToDesktop",
  "cancelCopy",
];

export const commands = Object.fromEntries(
  COMMAND_NAMES.map((name) => [name, vi.fn()]),
) as unknown as MockedCommands;

type Handler<T> = (payload: T) => void;
const listeners = {
  scanProgress: new Set<Handler<ScanProgress>>(),
  copyProgress: new Set<Handler<CopyProgress>>(),
  connectionLost: new Set<Handler<ConnectionLost>>(),
};

function subscribe<T>(set: Set<Handler<T>>) {
  return (handler: Handler<T>) => {
    set.add(handler);
    return Promise.resolve(() => {
      set.delete(handler);
    });
  };
}

export const events = {
  scanProgress: vi.fn(subscribe(listeners.scanProgress)),
  copyProgress: vi.fn(subscribe(listeners.copyProgress)),
  connectionLost: vi.fn(subscribe(listeners.connectionLost)),
};

/** What `vi.mock("../api", ...)` swaps in. */
export const apiMock = { commands, events };

/** Delivers an event to every current subscriber, like the host's emit. */
export const emit = {
  scanProgress: (p: ScanProgress) => listeners.scanProgress.forEach((h) => h(p)),
  copyProgress: (p: CopyProgress) => listeners.copyProgress.forEach((h) => h(p)),
  connectionLost: (l: ConnectionLost) => listeners.connectionLost.forEach((h) => h(l)),
};

/** How many listeners are subscribed to each event, to prove they are cleaned up. */
export const listenerCount = () => ({
  scanProgress: listeners.scanProgress.size,
  copyProgress: listeners.copyProgress.size,
  connectionLost: listeners.connectionLost.size,
});

/** Call from `beforeEach`: forgets every call and answer, and drops all subscribers. */
export function resetApi() {
  for (const name of COMMAND_NAMES) commands[name].mockReset();
  events.scanProgress.mockReset().mockImplementation(subscribe(listeners.scanProgress));
  events.copyProgress.mockReset().mockImplementation(subscribe(listeners.copyProgress));
  events.connectionLost.mockReset().mockImplementation(subscribe(listeners.connectionLost));
  listeners.scanProgress.clear();
  listeners.copyProgress.clear();
  listeners.connectionLost.clear();

  // Commands that answer with nothing and are called without being awaited.
  commands.listDevices.mockResolvedValue([]);
  commands.focus.mockResolvedValue(undefined);
  commands.cancelScan.mockResolvedValue(undefined);
  commands.cancelCopy.mockResolvedValue(undefined);
  commands.disconnect.mockResolvedValue(undefined);
}

// ── Fixtures ────────────────────────────────────────────────────────────────

export const SESSION = 1;

export function node(id: number, session = SESSION): NodeRef {
  return { session, id };
}

export function size(bytes: number, apparent = bytes): Size {
  return { apparent, disk: bytes };
}

export function totals(over: Partial<Totals> = {}): Totals {
  return { size: size(100), files: 3, dirs: 2, errors: 0, ...over };
}

export function row(id: number, over: Partial<Row> = {}): Row {
  return {
    node: node(id, over.node?.session ?? SESSION),
    name: `item${id}`,
    kind: "file",
    size: size(10),
    measure: "final",
    files: 1,
    unreadable: false,
    parent: null,
    parentNode: null,
    ...over,
  };
}

export function view(id: number, rows: Row[] = [], over: Partial<View> = {}): View {
  return {
    node: node(id, over.node?.session ?? SESSION),
    path: `/dir${id}`,
    size: size(100),
    measure: "final",
    files: rows.length,
    dirs: 0,
    unreadable: false,
    rows,
    hidden: 0,
    ...over,
  };
}

export function treemapNode(id: number, over: Partial<TreemapNode> = {}): TreemapNode {
  return {
    node: node(id, over.node?.session ?? SESSION),
    name: `node${id}`,
    kind: "dir",
    size: size(100),
    measure: "final",
    children: [],
    ...over,
  };
}

export function connected(over: Partial<Connected> = {}): Connected {
  return { serial: "s", model: "Pixel", root: "/sdcard", previous: null, ...over };
}

export function started(session = SESSION): ScanStarted {
  return { session, root: node(0, session), path: "/sdcard" };
}

/** A `scan-progress` payload; by default the skeleton just became ready. */
export function progress(over: Partial<ScanProgress> = {}): ScanProgress {
  return {
    session: SESSION,
    phase: "measuring",
    totals: totals(),
    listed: 5,
    known: 20,
    elapsedMs: 1200,
    skeleton: { listed: 5, frontier: 15, elapsedMs: 300 },
    focus: null,
    finished: null,
    view: null,
    ...over,
  };
}

export function commandError(code: ErrorCode, message: string): CommandError {
  return { code, message };
}

export function copyProgress(over: Partial<CopyProgress> = {}): CopyProgress {
  return {
    node: node(2),
    destination: "C:\\Users\\me\\Desktop\\item2",
    filesDone: 1,
    filesTotal: 4,
    bytesDone: 1024,
    bytesTotal: 4096,
    totalsKnown: true,
    current: "a.jpg",
    ...over,
  };
}

/** A promise the test settles by hand, to put a reply "in flight". */
export function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}
