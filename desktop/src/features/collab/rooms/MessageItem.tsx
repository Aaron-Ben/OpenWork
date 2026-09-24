import { CornerUpLeft } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { CollabRoomMessage } from '@/bridge/collab'
import { CopyButton } from '@/components/ui/CopyButton'
import { identityClasses } from '@/features/collab/components/agentIdentity'
import { ParticipantAvatar } from '@/features/collab/components/ParticipantAvatar'
import { formatBeijingClock } from '@/lib/dateTime'
import { cn } from '@/lib/utils'
import { CardSummary } from './CardLinks'
import { MessageBody } from './MessageBody'
import { cardIdsOutsideCode } from './messageText'
import type { FoundCard } from './roomTimeline'

/** 渲染一条消息需要的房间上下文。 */
export interface MessageContext {
  knownIds: ReadonlySet<string>
  agentNames: ReadonlyMap<string, string>
  findCard: (cardId: string) => FoundCard | null
  selectedCardId: string | null
  highlightedMessageId: string | null
  onOpenCard: (cardId: string) => void
  onOpenAgent: (agentId: string) => void
  onJump: (messageId: string) => void
  /** 只读房间（Agent 私聊）不给引用回复。 */
  onQuote: ((message: CollabRoomMessage) => void) | null
}

/** 一条消息（collaboration-desktop.md §7.2）：头像、名字、role、时间、引用、正文、卡片摘要、悬停工具条。 */
export function MessageItem({ message, context }: { message: CollabRoomMessage, context: MessageContext }) {
  const { t } = useTranslation()
  const isUser = message.authorKind === 'user'
  const name = isUser ? t('collab.rooms.user') : message.authorName
  const summaries = cardIdsOutsideCode(message.body)
    .map((cardId) => ({ cardId, found: context.findCard(cardId) }))
    .filter((item): item is { cardId: string, found: FoundCard } => item.found !== null)
  const openAuthor = () => { if (!isUser) context.onOpenAgent(message.authorId) }

  return (
    <article
      data-message-id={message.id}
      className={cn(
        'group relative -mx-2.5 flex items-start gap-3 rounded-xl px-2.5 py-1.5 transition-colors',
        context.highlightedMessageId === message.id ? 'bg-clay-soft' : 'hover:bg-paper-hover',
      )}
    >
      <button type="button" disabled={isUser} aria-label={t('collab.rooms.viewProfile', { name })} className="rounded-full" onClick={openAuthor}>
        <ParticipantAvatar participantId={message.authorId} name={name} />
      </button>
      <div className="flex min-w-0 flex-col gap-1">
        <div className="flex items-baseline gap-2">
          {isUser
            ? <strong className="text-[13px]">{name}</strong>
            : <button type="button" className={cn('text-[13px] font-bold hover:underline', identityClasses(message.authorId).text)} onClick={openAuthor}>{name}</button>}
          {message.authorRole ? <span className="text-[11px] text-ink-soft">{message.authorRole}</span> : null}
          <span className="text-xs tabular-nums text-ink-soft">{formatBeijingClock(message.createdAt)}</span>
        </div>
        {message.quoted ? (
          <button
            type="button"
            className="flex max-w-[560px] items-center gap-2 rounded-lg border border-line bg-paper-hover px-2.5 py-1.5 text-left text-xs text-ink-soft"
            onClick={() => context.onJump(message.quoted?.id ?? '')}
          >
            <CornerUpLeft size={13} className="shrink-0" />
            <span className="truncate">
              {t('collab.rooms.lastMessage', {
                author: message.quoted.authorId === 'local-user' ? t('collab.rooms.user') : message.quoted.authorName,
                body: message.quoted.body,
              })}
            </span>
            <span className="shrink-0 text-[11px] text-ink-soft">{t('collab.rooms.jumpToQuote')}</span>
          </button>
        ) : null}
        <div className="max-w-[640px] text-sm leading-relaxed">
          <MessageBody
            body={message.body}
            knownIds={context.knownIds}
            findCard={context.findCard}
            selectedCardId={context.selectedCardId}
            onOpenCard={context.onOpenCard}
          />
        </div>
        {summaries.map(({ cardId, found }) => (
          <CardSummary
            key={cardId}
            cardId={cardId}
            found={found}
            assigneeName={found.card.assigneeId ? context.agentNames.get(found.card.assigneeId) ?? found.card.assigneeId : null}
            selected={cardId === context.selectedCardId}
            onOpen={context.onOpenCard}
          />
        ))}
      </div>
      <div role="toolbar" aria-label={t('collab.rooms.messageActions')} className="absolute -top-3.5 right-3 hidden gap-0.5 rounded-lg border border-line bg-surface p-0.5 shadow-sm group-hover:flex">
        {context.onQuote ? (
          <button type="button" className="flex h-7 items-center gap-1 rounded-md px-2 text-xs text-ink-soft hover:bg-paper-hover" onClick={() => context.onQuote?.(message)}>
            <CornerUpLeft size={13} />{t('collab.rooms.quoteReply')}
          </button>
        ) : null}
        <CopyButton text={message.body} className="grid size-7 place-items-center rounded-md text-ink-soft hover:bg-paper-hover" iconSize={14} />
      </div>
    </article>
  )
}
