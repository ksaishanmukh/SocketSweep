/**
 * A row of mutually exclusive buttons in one pill. `role` chooses the
 * semantics: tabs for switching screens, a radio group for a setting.
 */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  label,
  role = "radiogroup",
  titles,
}: {
  options: { value: T; label: string }[];
  value: T;
  onChange: (next: T) => void;
  label: string;
  role?: "radiogroup" | "tablist";
  titles?: Partial<Record<T, string>>;
}) {
  const itemRole = role === "tablist" ? "tab" : "radio";
  return (
    <div
      role={role}
      aria-label={label}
      className="inline-flex rounded-lg bg-sunk border border-line p-0.5"
    >
      {options.map((o) => {
        const active = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role={itemRole}
            aria-checked={itemRole === "radio" ? active : undefined}
            aria-selected={itemRole === "tab" ? active : undefined}
            title={titles?.[o.value]}
            onClick={() => onChange(o.value)}
            className={`min-h-8 px-3 rounded-md text-[13px] font-medium transition-colors ${
              active ? "bg-panel text-fg shadow-sm" : "text-muted hover:text-fg"
            }`}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
