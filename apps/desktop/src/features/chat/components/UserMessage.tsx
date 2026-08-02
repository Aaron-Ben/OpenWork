import { memo } from 'react'

import type { ContentBlock, TextBlock } from '@/types/parts'

interface UserMessageProps {
  parts: ContentBlock[]
}

export const UserMessage = memo(function UserMessage({ parts }: UserMessageProps) {
  const text = parts
    .filter((part): part is TextBlock => part.type === 'text')
    .map((part) => part.text)
    .join('\n')
  if (!text.trim()) return null

  return (
    <div className="flex justify-end pb-1 pt-5">
      <div className="min-w-0 max-w-[85%] rounded-2xl bg-paper-hover px-4 py-2.5 text-sm leading-relaxed text-ink whitespace-pre-wrap break-words [overflow-wrap:anywhere] [word-break:break-word] sm:max-w-[75%]">
        {text}
      </div>
    </div>
  )
})
