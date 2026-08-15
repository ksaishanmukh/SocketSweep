import { useCallback, useEffect, useRef, useState } from "react";
import { Treemap, ResponsiveContainer, Tooltip as RechartsTooltip } from "recharts";
import { formatBytes, formatNumber } from "./lib/format";
import * as ipc from "./lib/ipc";
import { ROOT_ID, type Crumb, type Row, type Stats, type View } from "./lib/types";
import "./App.css";

// ── Types ───────────────────────────────────────────────────────────────────

interface Toast {
  id: number;
  message: string;
  type: "error" | "success" | "info";
  exiting?: boolean;
}

type AppPhase = "setup" | "connecting" | "scanning" | "result";

// ── Icons (inline SVG) ──────────────────────────────────────────────────────

function IconFolder({ className = "" }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 16 16" fill="none">
      <path
        d="M1.5 3C1.5 2.44772 1.94772 2 2.5 2H6.29289L7.64645 3.35355L7.85355 3.5H8H13.5C14.0523 3.5 14.5 3.94772 14.5 4.5V12.5C14.5 13.0523 14.0523 13.5 13.5 13.5H2.5C1.94772 13.5 1.5 13.0523 1.5 12.5V3Z"
        fill="currentColor"
        opacity="0.2"
        stroke="currentColor"
        strokeWidth="1"
      />
    </svg>
  );
}

function IconFile({ className = "" }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 16 16" fill="none">
      <path
        d="M4 1.5h5.586L13 4.914V14a.5.5 0 01-.5.5h-8A.5.5 0 014 14V2a.5.5 0 01.5-.5z"
        stroke="currentColor"
        strokeWidth="1"
        fill="none"
      />
      <path d="M9.5 1.5V5H13" stroke="currentColor" strokeWidth="1" fill="none" />
    </svg>
  );
}

function IconUsb() {
  return (
    <svg
      width="24"
      height="24"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M12 22v-8m0 0V6m0 8l4-2v-2m-4 4l-4-2v-2" />
      <circle cx="12" cy="4" r="2" />
      <circle cx="8" cy="10" r="1" />
      <rect x="15" y="9" width="2" height="2" rx="0.5" />
    </svg>
  );
}

function IconTrash() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 14 14"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.2"
      strokeLinecap="round"
    >
      <path d="M2 3.5h10M5.5 3.5V2.5a1 1 0 011-1h1a1 1 0 011 1v1M3.5 3.5l.5 8.5a1 1 0 001 1h4a1 1 0 001-1l.5-8.5" />
      <path d="M5.5 6v4M8.5 6v4" />
    </svg>
  );
}

function Spinner({ size = 20, className = "" }: { size?: number; className?: string }) {
  return (
    <svg
      className={`animate-spin ${className}`}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
    >
      <circle cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="2.5" opacity="0.2" />
      <path
        d="M12 2a10 10 0 019.95 9"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
      />
    </svg>
  );
}

// ── Toast System ────────────────────────────────────────────────────────────

let toastId = 0;

function ToastContainer({
  toasts,
  onDismiss,
}: {
  toasts: Toast[];
  onDismiss: (id: number) => void;
}) {
  return (
    <div className="fixed top-4 right-4 z-50 flex flex-col gap-2 max-w-sm" id="toast-container">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={`
            ${t.exiting ? "animate-toast-out" : "animate-toast-in"}
            flex items-start gap-3 px-4 py-3 rounded-lg border shadow-xl cursor-pointer
            ${
              t.type === "error"
                ? "bg-red-950/80 border-red-800/50 text-red-200"
                : t.type === "success"
                  ? "bg-emerald-950/80 border-emerald-800/50 text-emerald-200"
                  : "bg-zinc-800/80 border-zinc-700/50 text-zinc-200"
            }
            backdrop-blur-md
          `}
          onClick={() => onDismiss(t.id)}
        >
          <span className="mt-0.5 text-base">
            {t.type === "error" ? "✕" : t.type === "success" ? "✓" : "ℹ"}
          </span>
          <p className="text-sm leading-relaxed flex-1">{t.message}</p>
        </div>
      ))}
    </div>
  );
}

// ── Terminal Log ────────────────────────────────────────────────────────────

/** Capped so a long session cannot grow it without bound. */
const MAX_LOG_LINES = 200;

function TerminalLog({ logs }: { logs: string[] }) {
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  if (logs.length === 0) return null;

  return (
    <div className="h-32 border-t border-zinc-800 bg-zinc-950 px-4 py-2 font-mono text-[11px] text-zinc-400 overflow-y-auto">
      {logs.map((log, i) => (
        <div key={i} className="mb-0.5 break-all">
          <span className="text-accent-500 mr-2">❯</span>
          {log}
        </div>
      ))}
      <div ref={endRef} />
    </div>
  );
}

// ── Setup Screen ────────────────────────────────────────────────────────────

function SetupScreen({ onConnect, loading }: { onConnect: () => void; loading: boolean }) {
  return (
    <div className="flex flex-col items-center justify-center flex-1 gap-8 animate-fade-in-up">
      <div className="flex flex-col items-center gap-6">
        <div
          className={`
          w-24 h-24 rounded-2xl bg-gradient-to-br from-accent-500/20 to-accent-700/10
          border border-accent-500/20 flex items-center justify-center
          ${loading ? "animate-pulse-glow" : ""}
        `}
        >
          <span className="text-accent-400">
            <IconUsb />
          </span>
        </div>

        <div className="text-center">
          <h1 className="text-3xl font-bold tracking-tight bg-gradient-to-r from-zinc-100 to-zinc-400 bg-clip-text text-transparent">
            SocketSweep
          </h1>
          <p className="mt-2 text-sm text-zinc-500 max-w-xs leading-relaxed">
            High-performance Android storage analyzer.
            <br />
            <span className="text-zinc-600">Bypasses MTP — direct POSIX scanning.</span>
          </p>
        </div>
      </div>

      <button
        id="btn-connect"
        onClick={onConnect}
        disabled={loading}
        className={`
          group relative px-8 py-3 rounded-xl font-semibold text-sm
          transition-all duration-300 cursor-pointer
          ${
            loading
              ? "bg-zinc-800 text-zinc-500 cursor-wait"
              : "bg-gradient-to-r from-accent-600 to-accent-500 text-white hover:from-accent-500 hover:to-accent-400 hover:shadow-lg hover:shadow-accent-500/20 hover:-translate-y-0.5"
          }
        `}
      >
        {loading ? (
          <span className="flex items-center gap-2">
            <Spinner size={16} />
            Connecting…
          </span>
        ) : (
          <span className="flex items-center gap-2">
            <IconUsb />
            Connect Device
          </span>
        )}
      </button>

      {/* ADB is bundled with the app, so it is deliberately not listed here. */}
      <div className="flex flex-col items-center gap-1.5 text-xs text-zinc-600">
        <p>• Android device connected via USB</p>
        <p>• USB Debugging enabled in Developer Options</p>
      </div>
    </div>
  );
}

// ── Scanning Screen ─────────────────────────────────────────────────────────

/**
 * Live counts rather than a static string. A scan takes several seconds and the
 * whole pitch of the app is speed, so this is the moment worth showing.
 */
function ScanningScreen({ stats, current }: { stats: Stats | null; current: string }) {
  return (
    <div className="flex flex-col items-center justify-center flex-1 gap-8 animate-fade-in-up">
      <div className="relative w-32 h-32">
        <div className="absolute inset-0 rounded-full border-2 border-accent-500/20" />
        <svg
          className="absolute inset-0 animate-spin"
          style={{ animationDuration: "2s" }}
          viewBox="0 0 128 128"
        >
          <circle
            cx="64"
            cy="64"
            r="62"
            fill="none"
            stroke="url(#scanGrad)"
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeDasharray="120 280"
          />
          <defs>
            <linearGradient id="scanGrad" x1="0%" y1="0%" x2="100%" y2="0%">
              <stop offset="0%" stopColor="oklch(0.60 0.18 180)" />
              <stop offset="100%" stopColor="oklch(0.60 0.18 180 / 0)" />
            </linearGradient>
          </defs>
        </svg>
        <div className="absolute inset-0 flex items-center justify-center">
          <div className="w-16 h-16 rounded-xl bg-accent-500/10 border border-accent-500/20 flex items-center justify-center scan-shimmer">
            <span className="text-accent-400 text-xl">⚡</span>
          </div>
        </div>
      </div>

      <div className="text-center">
        <h2 className="text-lg font-semibold text-zinc-200">Scanning Storage</h2>
        <p className="mt-2 text-2xl font-mono font-bold text-accent-400 tabular-nums">
          {stats ? formatBytes(stats.size) : "—"}
        </p>
        <p className="mt-1 text-sm text-zinc-500 font-mono tabular-nums">
          {stats ? `${formatNumber(stats.files)} files · ${formatNumber(stats.dirs)} folders` : ""}
        </p>
        <p className="mt-2 text-xs text-zinc-600 font-mono truncate max-w-sm">{current}</p>
      </div>
    </div>
  );
}

// ── Size Bar ────────────────────────────────────────────────────────────────

/**
 * Proportional to the total on screen, in a single hue.
 *
 * The previous version measured each row against its immediate parent and
 * mapped that to red/amber/teal, which made the root row permanently red and
 * gave "red" no meaning. Red now belongs to destructive actions only.
 */
function SizeBar({ ratio }: { ratio: number }) {
  const pct = Math.max(0.5, Math.min(1, ratio) * 100);
  return (
    <div className="w-24 h-1.5 rounded-full bg-zinc-800 overflow-hidden flex-shrink-0">
      <div
        className="h-full rounded-full size-bar-inner bg-accent-500"
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

// ── Row ─────────────────────────────────────────────────────────────────────

function FileRow({
  row,
  totalSize,
  onOpen,
  onDelete,
}: {
  row: Row;
  totalSize: number;
  onOpen: (row: Row) => void;
  onDelete: (row: Row) => void;
}) {
  const ratio = totalSize > 0 ? row.size / totalSize : 0;

  return (
    <div
      className="file-row flex items-center gap-3 px-3 py-1.5 rounded-md group select-none"
      onDoubleClick={() => row.isDir && onOpen(row)}
    >
      <span className={row.isDir ? "text-accent-400" : "text-zinc-500"}>
        {row.isDir ? <IconFolder /> : <IconFile />}
      </span>

      {row.isDir ? (
        <button
          className="flex-1 truncate text-left text-sm text-zinc-200 font-medium hover:text-accent-300 focus-visible:outline-2 focus-visible:outline-accent-500 rounded"
          onClick={() => onOpen(row)}
        >
          {row.name}
        </button>
      ) : (
        <span className="flex-1 truncate text-sm text-zinc-400">{row.name}</span>
      )}

      {!row.complete && (
        <span
          className="text-[10px] uppercase tracking-wider text-zinc-600"
          title="Still being scanned"
        >
          …
        </span>
      )}

      <SizeBar ratio={ratio} />

      <span className="w-20 text-right text-xs font-mono text-zinc-500 flex-shrink-0 tabular-nums">
        {formatBytes(row.size)}
      </span>

      {/*
        Visible on focus as well as hover: the previous version was
        opacity-0/group-hover only, so keyboard users could not reach it at all.
      */}
      <button
        className="btn-nuke opacity-0 group-hover:opacity-100 focus-visible:opacity-100 ml-1 px-2 py-1 rounded-md
          bg-danger-600/20 border border-danger-500/30 text-danger-400
          hover:bg-danger-500/30 text-[10px] uppercase font-bold tracking-wider flex items-center gap-1
          cursor-pointer focus-visible:outline-2 focus-visible:outline-danger-500"
        onClick={() => onDelete(row)}
        title={`Delete ${row.name}`}
        aria-label={`Delete ${row.name}`}
      >
        <IconTrash />
        <span className="hidden sm:inline">Delete</span>
      </button>
    </div>
  );
}

// ── Stat strip ──────────────────────────────────────────────────────────────

function StatStrip({ stats, elapsedMs }: { stats: Stats; elapsedMs: number | null }) {
  return (
    <div className="flex items-center gap-4 px-6 py-3 text-sm shrink-0 border-b border-zinc-800/60">
      <span className="font-mono font-bold text-accent-400 tabular-nums">
        {formatBytes(stats.size)}
      </span>
      <span className="text-zinc-600">·</span>
      <span className="text-zinc-400 tabular-nums">{formatNumber(stats.files)} files</span>
      <span className="text-zinc-600">·</span>
      <span className="text-zinc-400 tabular-nums">{formatNumber(stats.dirs)} folders</span>
      {elapsedMs !== null && (
        <>
          <span className="text-zinc-600">·</span>
          <span className="text-zinc-500 tabular-nums">{(elapsedMs / 1000).toFixed(1)}s</span>
        </>
      )}
    </div>
  );
}

// ── Treemap ─────────────────────────────────────────────────────────────────

const TREEMAP_COLORS = [
  "#0d9488",
  "#0284c7",
  "#4f46e5",
  "#7c3aed",
  "#c026d3",
  "#e11d48",
  "#ea580c",
  "#ca8a04",
];

// The index signature is Recharts' requirement, not ours — another small sign
// that this component is being used against its grain. Stage 5 replaces it.
interface TreemapCell {
  name: string;
  size: number;
  nodeId: number;
  isDir: boolean;
  [key: string]: string | number | boolean;
}

// Recharts hands its content renderer an untyped bag of layout props.
// Stage 5 replaces Recharts with a squarify function we own.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const TreemapContent = (props: any) => {
  const { x, y, width, height, index, name, value } = props;
  if (width < 30 || height < 30) return null;
  const bg = TREEMAP_COLORS[index % TREEMAP_COLORS.length];

  return (
    <g>
      <rect
        x={x}
        y={y}
        width={width}
        height={height}
        style={{ fill: bg, stroke: "#ffffff20", strokeWidth: 1 }}
      />
      {width > 50 && height > 30 && name && (
        <text x={x + 6} y={y + 18} fill="#fff" fontSize={11} className="pointer-events-none">
          {name}
        </text>
      )}
      {width > 50 && height > 45 && value !== undefined && (
        <text
          x={x + 6}
          y={y + 32}
          fill="#94a3b8"
          fontSize={9}
          className="font-mono pointer-events-none"
        >
          {formatBytes(value)}
        </text>
      )}
    </g>
  );
};

// ── Result Screen ───────────────────────────────────────────────────────────

function ResultScreen({
  view,
  crumbs,
  stats,
  elapsedMs,
  onOpen,
  onRescan,
  onDisconnect,
  onDelete,
}: {
  view: View;
  crumbs: Crumb[];
  stats: Stats;
  elapsedMs: number | null;
  onOpen: (id: number) => void;
  onRescan: () => void;
  onDisconnect: () => void;
  onDelete: (row: Row) => void;
}) {
  const cells: TreemapCell[] = view.rows
    .filter((r) => r.size > 0)
    .slice(0, 40)
    .map((r) => ({ name: r.name, size: r.size, nodeId: r.id, isDir: r.isDir }));

  return (
    <div className="flex flex-col h-full animate-fade-in-up min-h-0">
      <header className="flex items-center justify-between px-6 py-3 border-b border-zinc-800/80 shrink-0">
        <div className="flex items-center gap-3 min-w-0">
          <div
            className={`w-2 h-2 rounded-full shrink-0 ${stats.scanning ? "bg-amber-500 animate-pulse" : "bg-emerald-500"}`}
          />
          <nav
            aria-label="Breadcrumb"
            className="flex items-center text-xs text-zinc-500 font-mono min-w-0"
          >
            {crumbs.map((crumb, idx) => (
              <span key={crumb.id} className="inline-flex items-center min-w-0">
                <button
                  onClick={() => onOpen(crumb.id)}
                  className={`hover:text-zinc-300 transition-colors truncate rounded focus-visible:outline-2 focus-visible:outline-accent-500 ${
                    idx === crumbs.length - 1 ? "text-zinc-300 font-semibold" : ""
                  }`}
                  aria-current={idx === crumbs.length - 1 ? "page" : undefined}
                >
                  {crumb.name}
                </button>
                {idx < crumbs.length - 1 && <span className="mx-2 opacity-50 shrink-0">/</span>}
              </span>
            ))}
          </nav>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          <button
            id="btn-rescan"
            onClick={onRescan}
            className="px-3 py-1.5 rounded-lg text-xs font-medium bg-zinc-800 text-zinc-300
              hover:bg-zinc-700 hover:text-zinc-100 border border-zinc-700/50"
          >
            ↻ Rescan
          </button>
          <button
            id="btn-disconnect"
            onClick={onDisconnect}
            className="px-3 py-1.5 rounded-lg text-xs font-medium bg-zinc-800 text-zinc-400
              hover:bg-red-950/50 hover:text-red-400 border border-zinc-700/50"
          >
            Disconnect
          </button>
        </div>
      </header>

      <StatStrip stats={stats} elapsedMs={elapsedMs} />

      <div className="px-6 py-4 shrink-0 h-56">
        <div className="glass-card rounded-xl p-2 w-full h-full">
          <ResponsiveContainer width="100%" height="100%">
            <Treemap
              data={cells}
              dataKey="size"
              aspectRatio={4 / 3}
              stroke="#fff"
              content={<TreemapContent />}
              isAnimationActive={false}
              onClick={(e: unknown) => {
                const cell = e as TreemapCell | undefined;
                if (cell?.isDir && cell.nodeId !== undefined) onOpen(cell.nodeId);
              }}
            >
              <RechartsTooltip
                content={({ active, payload }) => {
                  if (!active || !payload?.length) return null;
                  const p = payload[0].payload as TreemapCell;
                  return (
                    <div className="bg-zinc-900 border border-zinc-800 p-2 rounded shadow-xl text-xs">
                      <p className="font-semibold text-zinc-200">{p.name}</p>
                      <p className="text-zinc-400 font-mono mt-1">{formatBytes(p.size)}</p>
                      {p.isDir && <p className="text-zinc-600 mt-1">Click to open</p>}
                    </div>
                  );
                }}
              />
            </Treemap>
          </ResponsiveContainer>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-3 pb-4 min-h-0">
        <div className="flex items-center gap-2 px-3 py-2 mb-1">
          <span className="text-xs font-semibold text-zinc-500 uppercase tracking-wider flex-1">
            {view.path}
          </span>
          {view.hidden > 0 && (
            <span className="text-xs text-zinc-600">and {formatNumber(view.hidden)} more</span>
          )}
        </div>

        {view.rows.length === 0 ? (
          <p className="px-3 py-8 text-center text-sm text-zinc-600">This folder is empty.</p>
        ) : (
          view.rows.map((row) => (
            <FileRow
              key={row.id}
              row={row}
              totalSize={view.size}
              onOpen={(r) => onOpen(r.id)}
              onDelete={onDelete}
            />
          ))
        )}
      </div>
    </div>
  );
}

// ── App ─────────────────────────────────────────────────────────────────────

function App() {
  const [phase, setPhase] = useState<AppPhase>("setup");
  const [stats, setStats] = useState<Stats | null>(null);
  const [view, setView] = useState<View | null>(null);
  const [crumbs, setCrumbs] = useState<Crumb[]>([]);
  const [elapsedMs, setElapsedMs] = useState<number | null>(null);
  const [scanningPath, setScanningPath] = useState("");
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [logs, setLogs] = useState<string[]>([]);
  const [confirmDelete, setConfirmDelete] = useState<Row | null>(null);
  const timerRefs = useRef<Map<number, ReturnType<typeof setTimeout>>>(new Map());

  const addLog = useCallback((msg: string) => {
    setLogs((prev) =>
      [...prev, `[${new Date().toLocaleTimeString()}] ${msg}`].slice(-MAX_LOG_LINES),
    );
  }, []);

  const pushToast = useCallback((message: string, type: Toast["type"] = "error") => {
    const id = ++toastId;
    setToasts((prev) => [...prev, { id, message, type }]);
    const timer = setTimeout(() => {
      setToasts((prev) => prev.map((t) => (t.id === id ? { ...t, exiting: true } : t)));
      setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), 300);
    }, 5000);
    timerRefs.current.set(id, timer);
  }, []);

  const dismissToast = useCallback((id: number) => {
    const timer = timerRefs.current.get(id);
    if (timer) {
      clearTimeout(timer);
      timerRefs.current.delete(id);
    }
    setToasts((prev) => prev.map((t) => (t.id === id ? { ...t, exiting: true } : t)));
    setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), 300);
  }, []);

  // Live scan updates. The host pushes only the view we are watching, so this
  // payload stays the same size no matter how large the device is.
  useEffect(() => {
    const unlisten = ipc.onScanProgress((p) => {
      setStats(p.stats);
      if (p.view) {
        setView(p.view);
        setScanningPath(p.view.path);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const timers = timerRefs.current;
    return () => timers.forEach((t) => clearTimeout(t));
  }, []);

  const openNode = useCallback(
    async (id: number) => {
      try {
        const [nextView, nextCrumbs] = await Promise.all([ipc.getView(id), ipc.getBreadcrumbs(id)]);
        setView(nextView);
        setCrumbs(nextCrumbs);
      } catch (err) {
        pushToast(ipc.errorMessage(err), "error");
      }
    },
    [pushToast],
  );

  const runScan = useCallback(async () => {
    setPhase("scanning");
    setElapsedMs(null);
    const started = performance.now();
    addLog("[SCAN] Walking /sdcard…");

    try {
      const finalStats = await ipc.scan();
      setStats(finalStats);
      setElapsedMs(performance.now() - started);
      await openNode(ROOT_ID);
      setPhase("result");
      addLog(
        `[SCAN] Done — ${formatNumber(finalStats.files)} files, ${formatBytes(finalStats.size)}.`,
      );
    } catch (err) {
      const message = ipc.errorMessage(err);
      pushToast(message, "error");
      addLog(`[ERROR] ${message}`);
      setPhase(view ? "result" : "setup");
    }
  }, [addLog, openNode, pushToast, view]);

  const handleConnect = useCallback(async () => {
    setPhase("connecting");
    try {
      addLog("[ADB] Looking for a device…");
      const info = await ipc.connect();
      addLog(`[ADB] Connected to ${info.model} (${info.serial}).`);
      addLog("[SOCKET] Daemon started on an abstract socket.");
      await runScan();
    } catch (err) {
      const message = ipc.errorMessage(err);
      pushToast(message, "error");
      addLog(`[ERROR] ${message}`);
      setPhase("setup");
    }
  }, [addLog, pushToast, runScan]);

  const handleDisconnect = useCallback(async () => {
    addLog("[SOCKET] Stopping the daemon…");
    try {
      await ipc.disconnect();
      pushToast("Disconnected", "info");
    } catch {
      addLog("[SOCKET] Daemon was already stopped.");
    }
    setView(null);
    setStats(null);
    setCrumbs([]);
    setPhase("setup");
  }, [addLog, pushToast]);

  const executeDelete = useCallback(async () => {
    if (!confirmDelete) return;
    const target = confirmDelete;
    setConfirmDelete(null);

    addLog(`[DELETE] Removing ${target.name}…`);
    try {
      const result = await ipc.deleteNode(target.id);
      setStats(result.stats);
      if (result.view) setView(result.view);
      pushToast(`Deleted ${target.name}`, "success");
      addLog(`[DELETE] Removed ${formatNumber(result.items)} items.`);
    } catch (err) {
      const message = ipc.errorMessage(err);
      pushToast(`Delete failed: ${message}`, "error");
      addLog(`[ERROR] ${message}`);
    }
  }, [addLog, confirmDelete, pushToast]);

  return (
    <div className="h-screen flex flex-col bg-surface-0 overflow-hidden">
      <ToastContainer toasts={toasts} onDismiss={dismissToast} />

      <div className="flex-1 flex flex-col min-h-0">
        {phase === "setup" && <SetupScreen onConnect={handleConnect} loading={false} />}
        {phase === "connecting" && <SetupScreen onConnect={handleConnect} loading={true} />}
        {phase === "scanning" && <ScanningScreen stats={stats} current={scanningPath} />}
        {phase === "result" && view && stats && (
          <ResultScreen
            view={view}
            crumbs={crumbs}
            stats={stats}
            elapsedMs={elapsedMs}
            onOpen={openNode}
            onRescan={runScan}
            onDisconnect={handleDisconnect}
            onDelete={setConfirmDelete}
          />
        )}
      </div>

      <TerminalLog logs={logs} />

      {confirmDelete && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 backdrop-blur-sm animate-fade-in-up">
          <div className="bg-zinc-900 border border-zinc-800 rounded-2xl p-6 max-w-sm w-full shadow-2xl">
            <div className="flex items-center gap-3 mb-4">
              <div className="w-10 h-10 rounded-full bg-red-500/10 flex items-center justify-center text-red-500">
                <IconTrash />
              </div>
              <h3 className="text-lg font-bold text-zinc-100">Delete permanently?</h3>
            </div>

            <p className="text-sm font-mono text-zinc-300 bg-zinc-950 p-2 rounded-lg break-all border border-zinc-800 mb-3">
              {confirmDelete.name}
            </p>

            {/*
              What will actually be lost. The host already knows the subtree
              totals, and this is the information that prevents mistakes — the
              previous dialog showed only the name.
            */}
            <p className="text-sm text-zinc-400 mb-6">
              This removes{" "}
              <span className="font-semibold text-zinc-200">{formatBytes(confirmDelete.size)}</span>
              {confirmDelete.isDir && (
                <>
                  {" "}
                  across{" "}
                  <span className="font-semibold text-zinc-200">
                    {formatNumber(confirmDelete.files)} files
                  </span>
                </>
              )}
              . It cannot be undone.
            </p>

            <div className="flex items-center gap-3 justify-end">
              <button
                onClick={() => setConfirmDelete(null)}
                className="px-4 py-2 rounded-lg text-sm font-medium text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={executeDelete}
                className="px-4 py-2 rounded-lg text-sm font-bold bg-red-600 hover:bg-red-500 text-white transition-colors shadow-lg shadow-red-500/20"
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
