const STEPS: { title: string; detail?: string }[] = [
  { title: "Plug it into this computer with a USB cable." },
  {
    title: "Turn on USB debugging.",
    detail:
      "Settings › About phone › tap Build number 7 times, then Developer options › USB debugging.",
  },
  { title: "Tap Allow when the phone asks." },
];

export function NoDeviceSteps() {
  return (
    <>
      <h2 className="text-[15px] font-semibold text-fg">Connect your phone</h2>
      <ol className="mt-4 flex flex-col gap-3.5">
        {STEPS.map((s, i) => (
          <li key={s.title} className="flex gap-3">
            <span className="w-[22px] h-[22px] rounded-full bg-sunk text-accent text-[12px] font-semibold flex items-center justify-center shrink-0">
              {i + 1}
            </span>
            <span className="text-[14px] text-fg">
              {s.title}
              {s.detail && <span className="block mt-0.5 text-[13px] text-muted">{s.detail}</span>}
            </span>
          </li>
        ))}
      </ol>
      <div className="mt-5 pt-4 border-t border-line flex items-center gap-2 text-[13px] text-muted">
        <span className="w-[7px] h-[7px] rounded-full bg-accent animate-pulse" aria-hidden />
        Looking for a phone…
      </div>
    </>
  );
}
