import { memo } from 'react'

import type { ChatItem, TurnPlanView } from '@/types/chat'
import { transcriptGapClass, type TranscriptGap } from '../transcriptSpacing'
import type { FileChangeView } from './FileDiffPanel'
import { AssistantMessage } from './AssistantMessage'
import { ToolActivityList } from './ToolActivityList'
import { UserMessage } from './UserMessage'

export interface TranscriptMessageProps {
  message: ChatItem
  highlighted: boolean
  /// 与上一条消息之间的间距。列表容器不设 gap,纵向节奏全部由这里决定。
  gap: TranscriptGap
  onOpenTrace: (turnId: string, providerToolCallId?: string) => void
  onUndoFileChanges: (changeIds: string[]) => Promise<void>
  onReapplyFileChanges: (changeIds: string[]) => Promise<void>
  onReviewFileChanges: (changes: FileChangeView[]) => void
}

function sameParts(left: ChatItem['parts'], right: ChatItem['parts']): boolean {
  return left === right || (
    left.length === right.length
    && left.every((part, index) => part === right[index])
  )
}

function sameMessageIds(left?: string[], right?: string[]): boolean {
  if (left === right) return true
  if (!left || !right || left.length !== right.length) return false
  return left.every((id, index) => id === right[index])
}

/**
 * 计划的比较必须逐字段做。
 *
 * transcript 每次重建都会新建包装对象,只比引用会让计划卡永远不刷新 —— 而 steps 本身
 * 来自 store / reducer,只有真的变了才换引用。
 */
function samePlan(left?: TurnPlanView, right?: TurnPlanView): boolean {
  if (left === right) return true
  if (!left || !right) return false
  return left.explanation === right.explanation
    && (left.steps === right.steps || (
      left.steps.length === right.steps.length
      && left.steps.every((step, index) =>
        step.step === right.steps[index].step && step.status === right.steps[index].status)
    ))
}

export function areTranscriptMessagePropsEqual(
  previous: TranscriptMessageProps,
  next: TranscriptMessageProps,
): boolean {
  return previous.highlighted === next.highlighted
    && previous.gap === next.gap
    && previous.onOpenTrace === next.onOpenTrace
    && previous.onUndoFileChanges === next.onUndoFileChanges
    && previous.onReapplyFileChanges === next.onReapplyFileChanges
    && previous.onReviewFileChanges === next.onReviewFileChanges
    && previous.message.id === next.message.id
    && previous.message.turnId === next.message.turnId
    && previous.message.role === next.message.role
    && sameMessageIds(previous.message.sourceMessageIds, next.message.sourceMessageIds)
    && previous.message.model === next.message.model
    && previous.message.isStreaming === next.message.isStreaming
    && previous.message.turnActive === next.message.turnActive
    && previous.message.isCompacting === next.message.isCompacting
    && previous.message.fileChangePresentation === next.message.fileChangePresentation
    && samePlan(previous.message.plan, next.message.plan)
    && sameParts(previous.message.parts, next.message.parts)
}

export const TranscriptMessage = memo(function TranscriptMessage({
  message,
  highlighted,
  gap,
  onOpenTrace,
  onUndoFileChanges,
  onReapplyFileChanges,
  onReviewFileChanges,
}: TranscriptMessageProps) {
  const openTrace = message.turnId
    ? (providerToolCallId?: string) => onOpenTrace(message.turnId!, providerToolCallId)
    : undefined
  const fileChangePresentation = message.fileChangePresentation ?? 'activity'
  const spacing = transcriptGapClass(gap)

  return (
    <div
      data-message-id={message.id}
      data-source-message-ids={message.sourceMessageIds?.join(' ')}
      data-transcript-gap={gap}
      data-turn-id={message.role === 'user' ? message.turnId ?? message.id : undefined}
      className={`rounded-xl transition-colors${spacing ? ` ${spacing}` : ''}${
        highlighted ? ' bg-clay-soft ring-2 ring-clay/45' : ''
      }`}
    >
      {message.role === 'user' ? (
        <UserMessage parts={message.parts} />
      ) : message.role === 'tool' ? (
        <ToolActivityList
          parts={message.parts}
          turnActive={message.turnActive ?? message.isStreaming === true}
          onOpenTrace={openTrace}
          onUndoFileChanges={onUndoFileChanges}
          onReapplyFileChanges={onReapplyFileChanges}
          onReviewFileChanges={onReviewFileChanges}
          fileChangePresentation={fileChangePresentation}
        />
      ) : (
        <AssistantMessage
          parts={message.parts}
          model={message.model}
          isStreaming={message.isStreaming}
          turnActive={message.turnActive ?? message.isStreaming === true}
          isCompacting={message.isCompacting}
          onOpenTrace={openTrace}
          onUndoFileChanges={onUndoFileChanges}
          onReapplyFileChanges={onReapplyFileChanges}
          onReviewFileChanges={onReviewFileChanges}
          fileChangePresentation={fileChangePresentation}
          plan={message.plan}
        />
      )}
    </div>
  )
}, areTranscriptMessagePropsEqual)
