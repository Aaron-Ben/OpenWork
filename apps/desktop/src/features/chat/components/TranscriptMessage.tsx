import { memo } from 'react'

import type { ChatItem } from '@/types/chat'
import { AssistantMessage } from './AssistantMessage'
import { ToolActivityList } from './ToolActivityList'
import { UserMessage } from './UserMessage'

export interface TranscriptMessageProps {
  message: ChatItem
  highlighted: boolean
  onOpenTrace: (turnId: string, providerToolCallId?: string) => void
  onUndoFileChanges: (changeIds: string[]) => Promise<void>
  onReapplyFileChanges: (changeIds: string[]) => Promise<void>
}

function sameParts(left: ChatItem['parts'], right: ChatItem['parts']): boolean {
  return left === right || (
    left.length === right.length
    && left.every((part, index) => part === right[index])
  )
}

export function areTranscriptMessagePropsEqual(
  previous: TranscriptMessageProps,
  next: TranscriptMessageProps,
): boolean {
  return previous.highlighted === next.highlighted
    && previous.onOpenTrace === next.onOpenTrace
    && previous.onUndoFileChanges === next.onUndoFileChanges
    && previous.onReapplyFileChanges === next.onReapplyFileChanges
    && previous.message.id === next.message.id
    && previous.message.turnId === next.message.turnId
    && previous.message.role === next.message.role
    && previous.message.model === next.message.model
    && previous.message.isStreaming === next.message.isStreaming
    && previous.message.isCompacting === next.message.isCompacting
    && previous.message.fileChangePresentation === next.message.fileChangePresentation
    && sameParts(previous.message.parts, next.message.parts)
}

export const TranscriptMessage = memo(function TranscriptMessage({
  message,
  highlighted,
  onOpenTrace,
  onUndoFileChanges,
  onReapplyFileChanges,
}: TranscriptMessageProps) {
  const openTrace = message.turnId
    ? (providerToolCallId?: string) => onOpenTrace(message.turnId!, providerToolCallId)
    : undefined
  const fileChangePresentation = message.fileChangePresentation ?? 'activity'

  return (
    <div
      data-message-id={message.id}
      data-turn-id={message.role === 'user' ? message.turnId ?? message.id : undefined}
      className={highlighted
        ? 'rounded-xl bg-clay-soft ring-2 ring-clay/45 transition-colors'
        : 'rounded-xl transition-colors'}
    >
      {message.role === 'user' ? (
        <UserMessage parts={message.parts} />
      ) : message.role === 'tool' ? (
        <ToolActivityList
          parts={message.parts}
          onOpenTrace={openTrace}
          onUndoFileChanges={onUndoFileChanges}
          onReapplyFileChanges={onReapplyFileChanges}
          fileChangePresentation={fileChangePresentation}
        />
      ) : (
        <AssistantMessage
          parts={message.parts}
          model={message.model}
          isStreaming={message.isStreaming}
          isCompacting={message.isCompacting}
          onOpenTrace={openTrace}
          onUndoFileChanges={onUndoFileChanges}
          onReapplyFileChanges={onReapplyFileChanges}
          fileChangePresentation={fileChangePresentation}
        />
      )}
    </div>
  )
}, areTranscriptMessagePropsEqual)
