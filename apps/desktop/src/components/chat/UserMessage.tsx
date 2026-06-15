import { memo } from 'react'

interface UserMessageProps {
  content: string
}

export const UserMessage = memo(function UserMessage({ content }: UserMessageProps) {
  const hasText = content.trim().length > 0
  if (!hasText) return null

  return (
    <div className="mb-5 flex justify-end">
      <div className="group flex min-w-0 max-w-[82%] flex-col items-end sm:max-w-[78%] lg:max-w-[72%]">
        <div
          className="min-w-0 max-w-full rounded-[18px_4px_18px_18px] bg-clay-soft px-4 py-3 text-sm leading-relaxed text-ink whitespace-pre-wrap break-words [overflow-wrap:anywhere] [word-break:break-word]"
        >
          {content}
        </div>
      </div>
    </div>
  )
})
