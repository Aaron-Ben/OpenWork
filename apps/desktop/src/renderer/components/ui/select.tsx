import { Select as SelectPrimitive } from "radix-ui";
import type { ComponentProps } from "react";
import { cn } from "../../lib/cn";
import { fieldClass } from "./input";

export const Select = SelectPrimitive.Root;
export const SelectValue = SelectPrimitive.Value;

export function SelectTrigger({ className, children, ...props }: ComponentProps<typeof SelectPrimitive.Trigger>) {
  return (
    <SelectPrimitive.Trigger
      className={cn(
        fieldClass,
        "flex h-8 items-center justify-between font-mono text-xs data-placeholder:text-faint",
        className,
      )}
      {...props}
    >
      {children}
      <SelectPrimitive.Icon className="text-faint">▾</SelectPrimitive.Icon>
    </SelectPrimitive.Trigger>
  );
}

export function SelectContent({ className, children, ...props }: ComponentProps<typeof SelectPrimitive.Content>) {
  return (
    <SelectPrimitive.Portal>
      <SelectPrimitive.Content
        position="popper"
        sideOffset={6}
        className={cn(
          "z-50 max-h-72 w-(--radix-select-trigger-width) overflow-hidden rounded-md border border-line-strong bg-raised p-1 text-text shadow-xl",
          className,
        )}
        {...props}
      >
        <SelectPrimitive.Viewport>{children}</SelectPrimitive.Viewport>
      </SelectPrimitive.Content>
    </SelectPrimitive.Portal>
  );
}

export function SelectItem({ className, children, ...props }: ComponentProps<typeof SelectPrimitive.Item>) {
  return (
    <SelectPrimitive.Item
      className={cn(
        "flex cursor-default items-center justify-between rounded px-2 py-1.5 font-mono text-xs outline-none select-none data-highlighted:bg-hover data-[state=checked]:text-accent",
        className,
      )}
      {...props}
    >
      <SelectPrimitive.ItemText>{children}</SelectPrimitive.ItemText>
      <SelectPrimitive.ItemIndicator>✓</SelectPrimitive.ItemIndicator>
    </SelectPrimitive.Item>
  );
}
