import { cva, type VariantProps } from "class-variance-authority";
import type { ComponentProps } from "react";
import { cn } from "../../lib/cn";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-1.5 rounded font-mono font-medium whitespace-nowrap transition-colors outline-none focus-visible:ring-3 focus-visible:ring-accent-soft disabled:pointer-events-none disabled:opacity-45",
  {
    variants: {
      variant: {
        primary: "border border-accent bg-accent font-semibold text-accent-fg hover:brightness-110",
        secondary: "border border-line-strong bg-transparent text-text hover:bg-hover",
        ghost: "border border-transparent bg-transparent text-muted hover:bg-hover hover:text-text",
      },
      size: {
        md: "h-[30px] px-3 text-xs",
        sm: "h-6 px-2 text-[11px]",
      },
    },
    defaultVariants: { variant: "secondary", size: "md" },
  },
);

export function Button({
  className,
  variant,
  size,
  type = "button",
  ...props
}: ComponentProps<"button"> & VariantProps<typeof buttonVariants>) {
  return <button type={type} className={cn(buttonVariants({ variant, size }), className)} {...props} />;
}
