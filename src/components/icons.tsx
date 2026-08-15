export function IconFolder({ className = "" }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
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

export function IconFile({ className = "" }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
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

export function IconUsb() {
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
      aria-hidden
    >
      <path d="M12 22v-8m0 0V6m0 8l4-2v-2m-4 4l-4-2v-2" />
      <circle cx="12" cy="4" r="2" />
      <circle cx="8" cy="10" r="1" />
      <rect x="15" y="9" width="2" height="2" rx="0.5" />
    </svg>
  );
}

export function IconTrash() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 14 14"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.2"
      strokeLinecap="round"
      aria-hidden
    >
      <path d="M2 3.5h10M5.5 3.5V2.5a1 1 0 011-1h1a1 1 0 011 1v1M3.5 3.5l.5 8.5a1 1 0 001 1h4a1 1 0 001-1l.5-8.5" />
      <path d="M5.5 6v4M8.5 6v4" />
    </svg>
  );
}

export function IconSearch() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 14 14"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
      aria-hidden
    >
      <circle cx="6" cy="6" r="4.25" />
      <path d="M9.2 9.2 12.5 12.5" />
    </svg>
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
