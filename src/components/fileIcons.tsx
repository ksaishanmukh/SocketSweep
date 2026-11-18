/** A filled folder shape, tinted by the caller. */
export function IconFolder({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" aria-hidden>
      <path
        d="M1.5 3.5A1.5 1.5 0 0 1 3 2h3.2l1.5 1.5H13A1.5 1.5 0 0 1 14.5 5v7.5A1.5 1.5 0 0 1 13 14H3a1.5 1.5 0 0 1-1.5-1.5v-9Z"
        fill="currentColor"
      />
    </svg>
  );
}

/** An outlined page, so a file never reads as a folder even in the same colour. */
export function IconFile({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden>
      <path
        d="M4 1.75h5.25L12.5 5v9.25H4a.5.5 0 0 1-.5-.5V2.25a.5.5 0 0 1 .5-.5Z"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinejoin="round"
      />
      <path d="M9 1.75V5.25h3.5" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" />
    </svg>
  );
}
