import type { ReactNode } from "react";
import { cn } from "../lib/cn";
import type { StatusTone } from "../lib/status";

const toneClass: Record<StatusTone | "reconnecting", string> = {
  working: "text-accent before:shadow-[0_0_0_3px_var(--accent-soft)]",
  idle: "text-faint",
  error: "text-danger",
  reconnecting: "text-warn",
};

/** 一个圆点加一段文字：Agent 状态与连接状态共用。 */
export function StatusTag({
  tone,
  children,
  className,
}: {
  tone: StatusTone | "reconnecting";
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 font-mono text-xs before:size-[7px] before:rounded-full before:bg-current",
        toneClass[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}
