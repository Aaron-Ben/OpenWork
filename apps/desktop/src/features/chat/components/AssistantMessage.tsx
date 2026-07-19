import { memo } from 'react'
import { useTranslation } from 'react-i18next'

import { MarkdownRenderer } from '../../../components/markdown/MarkdownRenderer'
import type { ContentBlock } from '../../../type/parts'
import { ThinkingBlock } from './ThinkingBlock'
import { ToolActivityList } from './ToolActivityList'

interface AssistantMessageProps {
  parts: ContentBlock[]
  isStreaming?: boolean
  model?: string
  onOpenTrace?: (providerToolCallId?: string) => void
  onUndoFileChanges?: (changeIds: string[]) => Promise<void>
  onReapplyFileChanges?: (changeIds: string[]) => Promise<void>
  fileChangePresentation?: 'activity' | 'summary'
}

export const AssistantMessage = memo(function AssistantMessage({
  parts,
  isStreaming = false,
  model,
  onOpenTrace,
  onUndoFileChanges,
  onReapplyFileChanges,
  fileChangePresentation = 'activity',
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
  const showText = messageParts.length > 0 || (parts.length === 0 && isStreaming)

  return (
    <div className="group flex min-w-0 flex-col gap-1.5 py-0.5">
      {model ? <div className="text-xs text-ink-faint">{model}</div> : null}
      {showText ? (
        <div className="min-w-0">
          {messageParts.map((part, index) =>
            renderMessagePart(part, index, isStreaming, hasContent, documentLayout),
          )}
          {parts.length === 0 && isStreaming ? (
            <span className="text-sm text-ink-faint">{t('chat.waiting')}</span>
          ) : null}
        </div>
      ) : null}
      {toolParts.length > 0 ? (
        <ToolActivityList
          parts={toolParts}
          onOpenTrace={onOpenTrace ? (providerToolCallId) => onOpenTrace(providerToolCallId) : undefined}
          onUndoFileChanges={onUndoFileChanges}
          onReapplyFileChanges={onReapplyFileChanges}
          fileChangePresentation={fileChangePresentation}
        />
      ) : null}
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
