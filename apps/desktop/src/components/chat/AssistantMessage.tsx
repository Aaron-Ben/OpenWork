import { memo } from 'react'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import { ThinkingBlock } from './ThinkingBlock'
import { ToolCallBlock, type ToolCallState } from './ToolCallBlock'

interface AssistantMessageProps {
  content: string
  reasoningText?: string | null
  isStreaming?: boolean
  model?: string
  toolCalls?: ToolCallState[]
}

export const AssistantMessage = memo(function AssistantMessage({
  content,
  reasoningText,
  isStreaming = false,
  model,
  toolCalls,
}: AssistantMessageProps) {
  const hasContent = content.trim().length > 0
  const hasReasoning = !!reasoningText?.trim()
  const hasToolCalls = !!toolCalls && toolCalls.length > 0

  if (!hasContent && !hasReasoning && !isStreaming && !hasToolCalls) return null

  const documentLayout = shouldUseDocumentLayout(content)

  return (
    <div className="mb-5 flex justify-start">
      <div className={`group flex min-w-0 flex-col items-start ${documentLayout ? 'w-full max-w-full' : 'max-w-[88%] sm:max-w-[80%] lg:max-w-[72%]'}`}>
        <div
          className={`rounded-[20px] rounded-tl-lg border border-line bg-paper px-4 py-3 text-sm text-ink shadow-sm ${
            documentLayout ? 'w-full' : 'max-w-full'
          }`}
        >
          {model ? <div className="mb-2 text-xs text-ink-faint">{model}</div> : null}
          {hasReasoning || isStreaming ? <ThinkingBlock content={reasoningText ?? ''} isActive={isStreaming && !hasContent} /> : null}
          {hasToolCalls ? (
            <div className="mb-2 flex flex-col gap-1.5">
              {toolCalls!.map((toolCall) => (
                <ToolCallBlock key={toolCall.id} toolCall={toolCall} />
              ))}
            </div>
          ) : null}
          {hasContent ? (
            <MarkdownRenderer content={content} variant={documentLayout ? 'document' : 'default'} streaming={isStreaming} />
          ) : isStreaming ? (
            <span className="text-ink-faint">Waiting for response</span>
          ) : null}
        </div>
      </div>
    </div>
  )
})

function shouldUseDocumentLayout(content: string) {
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
