import type { ReactNode } from "react";
import { IconPhone } from "../../icons";

/** The phone tile, its name, and a caller-supplied line under the name. */
export function DeviceRow({
  model,
  tone = "accent",
  children,
}: {
  model: string;
  tone?: "accent" | "warn";
  children: ReactNode;
}) {
  return (
    <div className="flex items-center gap-3.5 min-w-0 text-left">
      <span
        className={`w-11 h-11 rounded-xl bg-sunk flex items-center justify-center shrink-0
          ${tone === "warn" ? "text-warn" : "text-accent"}`}
      >
        <IconPhone size={22} />
      </span>
      <span className="min-w-0">
        <span className="block text-[15px] font-semibold text-fg truncate">{model}</span>
        <span className="block mt-0.5 text-[12.5px] text-muted truncate">{children}</span>
      </span>
    </div>
  );
}

export function UsbLine({ serial }: { serial: string }) {
  return (
    <>
      <span className="inline-block w-[7px] h-[7px] rounded-full bg-ok mr-1.5" aria-hidden />
      Connected over USB · <span className="font-mono">{serial}</span>
    </>
  );
}
