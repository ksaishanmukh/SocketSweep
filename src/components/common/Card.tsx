import type { ReactNode } from "react";

/** A titled overview card. The heading names the region for screen readers. */
export function Card({
  id,
  title,
  aside,
  className = "",
  children,
}: {
  id: string;
  title: ReactNode;
  /** Shown at the right of the title row, e.g. a total. */
  aside?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section aria-labelledby={`${id}-title`} id={id} className={`card p-4 ${className}`}>
      <div className="flex items-baseline gap-3 mb-3">
        <h2 id={`${id}-title`} tabIndex={-1} className="text-[15px] font-semibold flex-1 min-w-0">
          {title}
        </h2>
        {aside}
      </div>
      {children}
    </section>
  );
}
