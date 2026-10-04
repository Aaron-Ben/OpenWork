import { Dialog as DialogPrimitive } from "radix-ui";
import type { ComponentProps } from "react";
import { cn } from "../../lib/cn";

export const Dialog = DialogPrimitive.Root;
export const DialogTrigger = DialogPrimitive.Trigger;
export const DialogClose = DialogPrimitive.Close;

/** 对话框本体：带遮罩，渲染到 body 末尾，Esc 与点击遮罩关闭。 */
export function DialogContent({ className, children, ...props }: ComponentProps<typeof DialogPrimitive.Content>) {
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className="fixed inset-0 z-40 bg-overlay" />
      <DialogPrimitive.Content
        className={cn(
          "fixed top-1/2 left-1/2 z-50 grid w-[440px] -translate-x-1/2 -translate-y-1/2 gap-3.5 rounded-[10px] border border-line-strong bg-bg px-6 py-[22px] text-text shadow-2xl outline-none",
          className,
        )}
        {...props}
      >
        {children}
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  );
}

export function DialogTitle({ className, ...props }: ComponentProps<typeof DialogPrimitive.Title>) {
  return <DialogPrimitive.Title className={cn("font-mono text-sm font-semibold", className)} {...props} />;
}

export function DialogDescription({ className, ...props }: ComponentProps<typeof DialogPrimitive.Description>) {
  return (
    <DialogPrimitive.Description className={cn("-mt-2 text-[13px] leading-relaxed text-muted", className)} {...props} />
  );
}
