import { memo } from 'react'

import { CopyButton } from '@/components/ui/CopyButton'
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
    <div className="group flex flex-col items-end gap-1.5 pb-1 pt-5">
      <div className="min-w-0 max-w-[85%] rounded-2xl bg-paper-hover px-4 py-2.5 text-sm leading-relaxed text-ink whitespace-pre-wrap break-words [overflow-wrap:anywhere] [word-break:break-word] sm:max-w-[75%]">
        {text}
      </div>
      {/*
        平时隐藏，悬停或键盘聚焦时出现。用 opacity 而不是条件渲染：后者会在悬停瞬间
        改变布局高度，把下方的消息挤得跳一下。focus-within 保证键盘用户也能拿到它。
      */}
      <CopyButton
        text={text}
        className="opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100"
      />
    </div>
  )
})
