import type { TextareaHTMLAttributes } from 'react'

import { cn } from '@/lib/utils'

export function Textarea({ className, ...props }: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      className={cn('w-full resize-none rounded-xl border border-line bg-paper px-3 py-2 text-sm text-ink outline-none placeholder:text-ink-faint focus:border-clay', className)}
      {...props}
    />
  )
}
