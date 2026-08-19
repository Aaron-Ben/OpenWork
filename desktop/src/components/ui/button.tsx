import { Slot } from '@radix-ui/react-slot'
import { cva, type VariantProps } from 'class-variance-authority'
import type { ButtonHTMLAttributes } from 'react'

import { cn } from '@/lib/utils'

const buttonVariants = cva(
  'inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors outline-none disabled:pointer-events-none disabled:opacity-45 focus-visible:ring-2 focus-visible:ring-clay/35',
  {
    variants: {
      variant: {
        default: 'bg-ink text-paper hover:bg-ink/85',
        /** clay 是全局唯一的 accent，留给"提交"这一类主动作。 */
        accent: 'bg-clay text-paper hover:bg-clay/88',
        outline: 'border border-line bg-paper text-ink hover:bg-paper-hover',
        destructive: 'bg-red-600 text-white hover:bg-red-700',
        ghost: 'bg-transparent text-ink-faint hover:bg-paper-hover hover:text-ink',
      },
      size: {
        default: 'h-9 px-4 py-2',
        sm: 'h-8 rounded-xl px-2.5',
        icon: 'size-9 rounded-full',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'default',
    },
  },
)

interface ButtonProps
  extends ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean
}

function Button({ className, variant, size, asChild = false, ...props }: ButtonProps) {
  const Comp = asChild ? Slot : 'button'
  return (
    <Comp
      data-slot="button"
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  )
}

export { Button, buttonVariants }
