import { memo } from 'react'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import { ThinkingBlock } from './ThinkingBlock'

interface AssistantMessageProps {
  content: string
  reasoningText?: string | null
  isStreaming?: boolean
  model?: string
}

export const AssistantMessage = memo(function AssistantMessage({
  content,
  reasoningText,
  isStreaming = false,
  model,
}: AssistantMessageProps) {
  const hasContent = content.trim().length > 0
  const hasReasoning = !!reasoningText?.trim()

  if (!hasContent && !hasReasoning && !isStreaming) return null

  const documentLayout = shouldUseDocumentLayout(content)

  return (
    <div className="mb-5 flex justify-start">
      <div className={`group flex min-w-0 flex-col items-start ${documentLayout ? 'w-full max-w-full' : 'max-w-[88%] sm:max-w-[80%] lg:max-w-[72%]'}`}>
        <div
          className={`rounded-[20px] rounded-tl-lg border border-slate-200 bg-white px-4 py-3 text-sm text-slate-800 shadow-sm ${
            documentLayout ? 'w-full' : 'max-w-full'
          }`}
        >
          {model ? <div className="mb-2 text-xs text-slate-400">{model}</div> : null}
          {hasReasoning || isStreaming ? <ThinkingBlock content={reasoningText ?? ''} isActive={isStreaming && !hasContent} /> : null}
          {hasContent ? (
            <MarkdownRenderer content={content} variant={documentLayout ? 'document' : 'default'} streaming={isStreaming} />
          ) : isStreaming ? (
            <span className="text-slate-400">Waiting for response</span>
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
