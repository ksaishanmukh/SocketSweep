import type { ReactNode } from "react";

/** A 16px line icon in the current text colour. Every icon is decorative; its button carries the name. */
function Line({ children, size = 16 }: { children: ReactNode; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {children}
    </svg>
  );
}

export function IconPhone({ size = 20 }: { size?: number }) {
  return (
    <Line size={size}>
      <rect x="4" y="1.5" width="8" height="13" rx="1.6" />
      <path d="M7 12.2h2" />
    </Line>
  );
}

export function IconTrash() {
  return (
    <Line>
      <path d="M2.5 4h11M6 4V2.75A.75.75 0 0 1 6.75 2h2.5a.75.75 0 0 1 .75.75V4M4 4l.6 9.1a1 1 0 0 0 1 .9h4.8a1 1 0 0 0 1-.9L12 4" />
      <path d="M6.5 7v4M9.5 7v4" />
    </Line>
  );
}

export function IconCopy() {
  return (
    <Line>
      <path d="M8 2v8M5 7l3 3 3-3M2.5 12.5v1h11v-1" />
    </Line>
  );
}

export function IconSearch() {
  return (
    <Line size={14}>
      <circle cx="7" cy="7" r="4.5" />
      <path d="m10.5 10.5 3 3" />
    </Line>
  );
}

export function IconFilter() {
  return (
    <Line>
      <path d="M2 3h12l-4.5 5.5V13l-3 1.5v-6L2 3Z" />
    </Line>
  );
}

export function IconBack() {
  return (
    <Line>
      <path d="M10 3 5 8l5 5" />
    </Line>
  );
}

export function IconForward() {
  return (
    <Line>
      <path d="m6 3 5 5-5 5" />
    </Line>
  );
}

export function IconClose({ size = 12 }: { size?: number }) {
  return (
    <Line size={size}>
      <path d="m4 4 8 8M12 4l-8 8" />
    </Line>
  );
}

export function IconRescan() {
  return (
    <Line>
      <path d="M13.5 8A5.5 5.5 0 1 1 11.9 4.1" />
      <path d="M13.5 2v3h-3" />
    </Line>
  );
}

export function Spinner({ size = 20, className = "" }: { size?: number; className?: string }) {
  return (
    <svg
      className={`animate-spin ${className}`}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden
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
