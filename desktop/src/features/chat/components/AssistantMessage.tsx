import { memo } from 'react'
import { LoaderCircle } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { MarkdownRenderer } from '@/components/markdown/MarkdownRenderer'
import { CopyButton } from '@/components/ui/CopyButton'
import type { ContentBlock } from '@/types/parts'
import type { TurnPlanView } from '@/types/chat'
import type { FileChangeView } from './FileDiffPanel'
import { PlanCard } from './PlanCard'
import { ThinkingBlock } from './ThinkingBlock'
import { ToolActivityList } from './ToolActivityList'

interface AssistantMessageProps {
  parts: ContentBlock[]
  isStreaming?: boolean
  turnActive?: boolean
  isCompacting?: boolean
  model?: string
  onOpenTrace?: (providerToolCallId?: string) => void
  onUndoFileChanges?: (changeIds: string[]) => Promise<void>
  onReapplyFileChanges?: (changeIds: string[]) => Promise<void>
  onReviewFileChanges?: (changes: FileChangeView[]) => void
  fileChangePresentation?: 'activity' | 'summary'
  plan?: TurnPlanView
  workspaceRoot?: string
}

export const AssistantMessage = memo(function AssistantMessage({
  parts,
  isStreaming = false,
  turnActive = isStreaming,
  isCompacting = false,
  model,
  onOpenTrace,
  onUndoFileChanges,
  onReapplyFileChanges,
  onReviewFileChanges,
  fileChangePresentation = 'activity',
  plan,
  workspaceRoot,
}: AssistantMessageProps) {
  const { t } = useTranslation()
  const messageParts = parts.filter((part) => part.type !== 'tool_call' && part.type !== 'tool_result')
  const hasContent = messageParts.some(
    (part) => part.type === 'text' && part.text.trim().length > 0,
  )
  // 只复制正文。thinking 是模型的草稿、工具调用有各自的复制入口，都不属于"这条回答"。
  const copyableText = parts
    .filter((part) => part.type === 'text')
    .map((part) => (part as { text: string }).text)
    .join('\n\n')

  if (parts.length === 0 && !isStreaming) return null

  const documentLayout = messageParts.some(
    (part) => part.type === 'text' && shouldUseDocumentLayout(part.text),
  )
  const showText = messageParts.length > 0 || (parts.length === 0 && isStreaming)

  return (
    /*
      根节点不带 py-*：消息两端的留白由 transcriptSpacing 统一给，否则相邻两条纯工具
      消息之间会被这里的内边距垫到 8px，跟同一条消息内的 2px 对不上。
    */
    <div className="group flex min-w-0 flex-col gap-1.5">
      {model ? <div className="text-xs text-ink-faint">{model}</div> : null}
      {showText ? (
        <div className="flex min-w-0 items-end gap-1.5">
          <div className="min-w-0 flex-1">
            {messageParts.map((part, index) =>
              renderMessagePart(part, index, isStreaming, hasContent, documentLayout),
            )}
            {parts.length === 0 && isStreaming ? (
              <span className="inline-flex items-center gap-2 text-sm text-ink-faint">
                {isCompacting ? <LoaderCircle size={14} className="animate-spin" /> : null}
                {isCompacting ? t('chat.commands.compacting') : t('chat.waiting')}
              </span>
            ) : null}
          </div>
          {/*
            复制入口钉在正文末行的右端。流式期间也要占位(invisible)：否则结束时多出
            这一列，正文末行可能被挤得重新折行。invisible 能压住 group-hover 的淡入，
            流式期间悬停也不会把只能复制半句话的按钮亮出来。
          */}
          {copyableText.trim() ? (
            <CopyButton
              text={copyableText}
              className={`opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100${isStreaming ? ' invisible' : ''}`}
            />
          ) : null}
        </div>
      ) : null}
      {parts.some((part) => part.type === 'tool_call' || part.type === 'tool_result') ? (
        <ToolActivityList
          parts={parts}
          turnActive={turnActive}
          onOpenTrace={onOpenTrace ? (providerToolCallId) => onOpenTrace(providerToolCallId) : undefined}
          onUndoFileChanges={onUndoFileChanges}
          onReapplyFileChanges={onReapplyFileChanges}
          onReviewFileChanges={onReviewFileChanges}
          fileChangePresentation={fileChangePresentation}
          workspaceRoot={workspaceRoot}
        />
      ) : null}
      {plan ? <PlanCard plan={plan} /> : null}
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
