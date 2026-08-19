import type { InputHTMLAttributes } from 'react'

import { cn } from '@/lib/utils'

export function Input({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={cn('h-9 w-full rounded-xl border border-line bg-paper px-3 text-sm text-ink outline-none placeholder:text-ink-faint focus:border-clay', className)}
      {...props}
    />
  )
}
