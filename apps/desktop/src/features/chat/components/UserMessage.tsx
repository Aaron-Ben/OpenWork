import { memo } from 'react'

import type { ContentBlock, TextBlock } from '../../../type/parts'

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
    <div className="mb-5 flex justify-end">
      <div className="group flex min-w-0 max-w-[82%] flex-col items-end sm:max-w-[78%] lg:max-w-[72%]">
        <div className="min-w-0 max-w-full rounded-[18px_4px_18px_18px] bg-clay-soft px-4 py-3 text-sm leading-relaxed text-ink whitespace-pre-wrap break-words [overflow-wrap:anywhere] [word-break:break-word]">
          {text}
        </div>
      </div>
    </div>
  )
})
