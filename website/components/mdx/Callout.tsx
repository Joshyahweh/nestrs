import clsx from "clsx";

type CalloutProps = {
  children: React.ReactNode;
  tone: "hint" | "warning" | "info";
};

const toneClass: Record<CalloutProps["tone"], string> = {
  hint: "border-l-4 border-emerald-400 bg-emerald-500/10 text-emerald-900 dark:text-emerald-100",
  warning: "border-l-4 border-amber-400 bg-amber-500/10 text-amber-900 dark:text-amber-100",
  info: "border-l-4 border-sky-400 bg-sky-500/10 text-sky-900 dark:text-sky-100"
};

export function Callout({ children, tone }: CalloutProps) {
  return (
    <aside className={clsx("my-6 rounded-r-lg p-4 text-sm leading-7", toneClass[tone])}>
      {children}
    </aside>
  );
}

export const Hint = ({ children }: { children: React.ReactNode }) => <Callout tone="hint">{children}</Callout>;
export const Warning = ({ children }: { children: React.ReactNode }) => <Callout tone="warning">{children}</Callout>;
export const Info = ({ children }: { children: React.ReactNode }) => <Callout tone="info">{children}</Callout>;
