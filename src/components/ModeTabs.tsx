import type { Mode } from "../hooks/useScanSession";

const TABS: { id: Mode; label: string; hint: string }[] = [
  { id: "treemap", label: "Treemap", hint: "Sizes as areas, drill down by folder" },
  { id: "largest", label: "Largest files", hint: "The biggest files anywhere on the device" },
  { id: "types", label: "File types", hint: "Storage grouped by kind of file" },
];

export function ModeTabs({ mode, onChange }: { mode: Mode; onChange: (m: Mode) => void }) {
  return (
    <div role="tablist" aria-label="Analysis" className="flex items-center gap-1">
      {TABS.map((tab) => (
        <button
          key={tab.id}
          role="tab"
          aria-selected={mode === tab.id}
          title={tab.hint}
          onClick={() => onChange(tab.id)}
          className={`px-2.5 py-1 rounded-md text-xs font-medium transition-colors
            focus-visible:outline-2 focus-visible:outline-accent-500 ${
              mode === tab.id
                ? "bg-accent-500/15 text-accent-300 border border-accent-500/30"
                : "text-zinc-500 hover:text-zinc-300 border border-transparent"
            }`}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
}
