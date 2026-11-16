import { Spinner } from "../../icons";

const STEPS = ["Phone connected", "Starting the scanner on the phone", "Reading storage"];

/** The host only reports two statuses, so the last step is "whatever comes after starting". */
function currentStep(status: string): number {
  if (/^connecting/i.test(status)) return 0;
  if (/^starting/i.test(status)) return 1;
  return 2;
}

export function ConnectingSteps({ status }: { status: string }) {
  const current = currentStep(status);
  return (
    <div className="mt-5">
      <p className="sr-only" role="status" aria-live="polite">
        {status}
      </p>
      <ol className="flex flex-col gap-3">
        {STEPS.map((label, i) => (
          <li key={label} className="flex items-center gap-3 text-[14px]">
            <StepMark state={i < current ? "done" : i === current ? "active" : "pending"} />
            <span className={i > current ? "text-faint" : "text-fg"}>{label}</span>
          </li>
        ))}
      </ol>
      <div className="mt-5 h-1 rounded-full bg-sunk overflow-hidden" aria-hidden>
        <div className="h-full rounded-full bg-accent indeterminate-bar" />
      </div>
    </div>
  );
}

function StepMark({ state }: { state: "done" | "active" | "pending" }) {
  if (state === "done") {
    return (
      <span className="w-[18px] h-[18px] rounded-full bg-ok flex items-center justify-center text-on-accent">
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden>
          <path
            d="m2 5.2 2 2 4-4.4"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </span>
    );
  }
  if (state === "active") return <Spinner size={18} className="text-accent" />;
  return <span className="w-[18px] h-[18px] rounded-full border-2 border-faint" aria-hidden />;
}
