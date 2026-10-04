import type { ComponentProps, ReactNode } from "react";
import { cn } from "../../lib/cn";

/** 输入框与文本域共用的外观；`aria-invalid` 为 true 时显示错误样式。 */
export const fieldClass =
  "w-full rounded border border-line-strong bg-raised px-2.5 py-1.5 font-sans text-[13px] leading-normal text-text placeholder:text-faint outline-none focus:border-accent focus:ring-3 focus:ring-accent-soft aria-invalid:border-danger aria-invalid:ring-3 aria-invalid:ring-danger-soft disabled:opacity-60";

export function Input({ className, ...props }: ComponentProps<"input">) {
  return <input className={cn(fieldClass, "h-8", className)} {...props} />;
}

export function Textarea({ className, ...props }: ComponentProps<"textarea">) {
  return <textarea className={cn(fieldClass, "min-h-[84px] resize-none", className)} {...props} />;
}

/** 表单的一项：标签、控件与下方的提示或错误。 */
export function Field({
  label,
  htmlFor,
  hint,
  error,
  children,
}: {
  label: string;
  htmlFor: string;
  hint?: string;
  error?: string;
  children: ReactNode;
}) {
  return (
    <div className="grid gap-1.5">
      <label htmlFor={htmlFor} className="font-mono text-[11px] text-muted">
        {label}
      </label>
      {children}
      {error ? (
        <p className="text-xs text-danger">{error}</p>
      ) : hint ? (
        <p className="text-xs text-muted">{hint}</p>
      ) : null}
    </div>
  );
}
