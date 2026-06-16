import { memo } from 'react'

import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import type { ContentBlock } from '../../type/parts'
import { ThinkingBlock } from './ThinkingBlock'
import { ToolCallBlock } from './ToolCallBlock'
import { ToolResultView } from './ToolResultView'

interface AssistantMessageProps {
  parts: ContentBlock[]
  isStreaming?: boolean
  model?: string
}

export const AssistantMessage = memo(function AssistantMessage({
  parts,
  isStreaming = false,
  model,
}: AssistantMessageProps) {
  const hasContent = parts.some((part) => part.type === 'text' && part.text.trim().length > 0)

  if (parts.length === 0 && !isStreaming) return null

  const documentLayout = parts.some(
    (part) => part.type === 'text' && shouldUseDocumentLayout(part.text),
  )

  return (
    <div className="mb-5 flex justify-start">
      <div
        className={`group flex min-w-0 flex-col items-start ${
          documentLayout ? 'w-full max-w-full' : 'max-w-[88%] sm:max-w-[80%] lg:max-w-[72%]'
        }`}
      >
        <div
          className={`rounded-[20px] rounded-tl-lg border border-line bg-paper px-4 py-3 text-sm text-ink shadow-sm ${
            documentLayout ? 'w-full' : 'max-w-full'
          }`}
        >
          {model ? <div className="mb-2 text-xs text-ink-faint">{model}</div> : null}
          {parts.map((part, index) =>
            renderPart(part, index, isStreaming, hasContent, documentLayout),
          )}
          {parts.length === 0 && isStreaming ? (
            <span className="text-ink-faint">Waiting for response</span>
          ) : null}
        </div>
      </div>
    </div>
  )
})

function renderPart(
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
      return (
        <div key={index} className="mb-2">
          <ToolCallBlock toolCall={part} />
        </div>
      )
    case 'tool_result':
      return (
        <div key={index} className="mb-2">
          <ToolResultView part={part} />
        </div>
      )
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
