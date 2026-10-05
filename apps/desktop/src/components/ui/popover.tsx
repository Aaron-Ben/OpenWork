import { Popover as PopoverPrimitive } from "radix-ui";
import type { ComponentProps } from "react";
import { cn } from "../../lib/cn";

export const Popover = PopoverPrimitive.Root;
export const PopoverTrigger = PopoverPrimitive.Trigger;
export const PopoverClose = PopoverPrimitive.Close;

/** 浮层本体：渲染到 body 末尾，Esc 与点击外面关闭。 */
export function PopoverContent({ className, ...props }: ComponentProps<typeof PopoverPrimitive.Content>) {
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Content
        sideOffset={8}
        className={cn(
          "z-50 w-[300px] rounded-xl border border-line-strong bg-raised p-3.5 text-[12.5px] text-text shadow-xl outline-none",
          className,
        )}
        {...props}
      />
    </PopoverPrimitive.Portal>
  );
}
