import { memo } from 'react'
import { useTranslation } from 'react-i18next'

import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import type { ContentBlock } from '../../type/parts'
import type { TurnTraceSummary } from '../../type/trace'
import { TurnTraceSummaryButton } from '../trace/TurnTraceSummaryButton'
import { ThinkingBlock } from './ThinkingBlock'
import { ToolActivityList } from './ToolActivityList'

interface AssistantMessageProps {
  parts: ContentBlock[]
  isStreaming?: boolean
  model?: string
  traceSummary?: TurnTraceSummary
  onOpenTrace?: (providerToolCallId?: string) => void
}

export const AssistantMessage = memo(function AssistantMessage({
  parts,
  isStreaming = false,
  model,
  traceSummary,
  onOpenTrace,
}: AssistantMessageProps) {
  const { t } = useTranslation()
  const messageParts = parts.filter((part) => part.type !== 'tool_call' && part.type !== 'tool_result')
  const toolParts = parts.filter((part) => part.type === 'tool_call' || part.type === 'tool_result')
  const hasContent = messageParts.some(
    (part) => part.type === 'text' && part.text.trim().length > 0,
  )

  if (parts.length === 0 && !isStreaming) return null

  const documentLayout = messageParts.some(
    (part) => part.type === 'text' && shouldUseDocumentLayout(part.text),
  )
  const showMessageCard = messageParts.length > 0 || (parts.length === 0 && isStreaming)

  return (
    <div className="mb-5 flex justify-start">
      <div
        className={`group flex min-w-0 flex-col items-start gap-2 ${
          documentLayout
            ? 'w-full max-w-full'
            : 'w-full max-w-[88%] sm:max-w-[80%] lg:max-w-[72%]'
        }`}
      >
        {showMessageCard ? (
          <div
            className={`rounded-[20px] rounded-tl-lg border border-line bg-paper px-4 py-3 text-sm text-ink shadow-sm ${
              documentLayout ? 'w-full' : 'max-w-full'
            }`}
          >
            {model ? <div className="mb-2 text-xs text-ink-faint">{model}</div> : null}
            {messageParts.map((part, index) =>
              renderMessagePart(part, index, isStreaming, hasContent, documentLayout),
            )}
            {parts.length === 0 && isStreaming ? (
              <span className="text-ink-faint">{t('chat.waiting')}</span>
            ) : null}
          </div>
        ) : null}
        {toolParts.length > 0 ? (
          <ToolActivityList
            parts={toolParts}
            onOpenTrace={onOpenTrace ? (providerToolCallId) => onOpenTrace(providerToolCallId) : undefined}
          />
        ) : null}
        {traceSummary && onOpenTrace ? (
          <TurnTraceSummaryButton summary={traceSummary} onOpen={() => onOpenTrace()} />
        ) : null}
      </div>
    </div>
  )
})

function renderMessagePart(
  part: ContentBlock,
  index: number,
  isStreaming: boolean,
  hasContent: boolean,
  documentLayout: boolean,
) {
  switch (part.type) {
    case 'thinking':
      return (
        <ThinkingBlock key={index} content={part.thinking} isActive={isStreaming && !hasContent} />
      )
    case 'text':
      return (
        <MarkdownRenderer
          key={index}
          content={part.text}
          variant={documentLayout ? 'document' : 'default'}
          streaming={isStreaming}
        />
      )
    case 'tool_call':
    case 'tool_result':
    case 'data':
      return null
    default:
      return null
  }
}

function shouldUseDocumentLayout(content: string): boolean {
  const normalized = content.trim()
  if (!normalized) return false
  if (/```/.test(normalized)) return true
  if (/^\s{0,3}(#{1,6}\s|[-*+]\s|\d+\.\s|>\s|\|.+\|)/m.test(normalized)) return true

  const paragraphs = normalized
    .split(/\n\s*\n/)
    .map((chunk) => chunk.trim())
    .filter(Boolean)

  return paragraphs.length >= 2 || normalized.split('\n').filter((line) => line.trim()).length >= 8
}
