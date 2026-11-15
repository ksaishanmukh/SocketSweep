import { useEffect, useRef } from "react";

/**
 * A real checkbox, in a 36px hit area so it is easy to land on. `mixed`
 * shows the indeterminate state, which only exists as a DOM property.
 */
export function Checkbox({
  checked,
  mixed = false,
  label,
  onChange,
}: {
  checked: boolean;
  mixed?: boolean;
  /** The accessible name; there is no visible label beside a row's box. */
  label: string;
  onChange: (checked: boolean) => void;
}) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = mixed;
  }, [mixed]);

  return (
    <label className="inline-flex items-center justify-center w-9 h-9 -m-2 cursor-pointer shrink-0">
      <input
        ref={ref}
        type="checkbox"
        checked={checked}
        aria-checked={mixed ? "mixed" : checked}
        aria-label={label}
        onChange={(e) => onChange(e.target.checked)}
        onClick={(e) => e.stopPropagation()}
      />
    </label>
  );
}
