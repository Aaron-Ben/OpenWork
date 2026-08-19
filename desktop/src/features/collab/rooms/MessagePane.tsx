import { ArrowDown, ArrowUp, AtSign, Bot, PanelRightClose, PanelRightOpen, Send, Sparkles, UserRound } from 'lucide-react'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabMessage, CollabRoomSummary } from '@/bridge/collab'
import { MarkdownRenderer } from '@/components/markdown/MarkdownRenderer'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { formatBeijingDateTime } from '@/lib/dateTime'
import { useCoordinationStore } from '@/features/collab/coordinationStore'
import { useCollabNavigationStore } from '@/features/collab/collabNavigationStore'
import { useMessageStore } from './messageStore'
import { useRoomStore } from './roomStore'

export function MessagePane({ room, rosterOpen, onToggleRoster }: {
  room: CollabRoomSummary
  rosterOpen: boolean
  onToggleRoster: () => void
}) {
  const { t } = useTranslation()
  const window = useMessageStore((state) => state.byRoom[room.id])
  const open = useMessageStore((state) => state.open)
  const loadOlder = useMessageStore((state) => state.loadOlder)
  const loadNewer = useMessageStore((state) => state.loadNewer)
  const send = useMessageStore((state) => state.send)
  const markRead = useRoomStore((state) => state.markRead)
  const held = useCoordinationStore((state) => state.heldByRoom[room.id])
  const navigate = useCollabNavigationStore((state) => state.navigate)
  const [draft, setDraft] = useState('')
  const viewportRef = useRef<HTMLDivElement>(null)
  const positionedRoom = useRef<string | null>(null)

  useEffect(() => {
    positionedRoom.current = null
    void open(room.id)
  }, [open, room.id])

  useLayoutEffect(() => {
    if (!window || positionedRoom.current === room.id) return
    const unreadSequence = room.lastReadSequence + 1
    const target = viewportRef.current?.querySelector(`[data-message-sequence="${unreadSequence}"]`)
    target?.scrollIntoView({ block: 'center' })
    positionedRoom.current = room.id
  }, [room.id, room.lastReadSequence, window])

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    const body = draft.trim()
    if (!body) return
    setDraft('')
    await send(room.id, body)
  }

  const enabledAgents = room.members.filter((member) => member.kind === 'agent' && member.enabled)
  const lastMessage = window?.messages[window.messages.length - 1]

  return (
    <section className="flex min-w-0 flex-1 flex-col bg-paper">
      <header data-tauri-drag-region="deep" className="flex h-12 shrink-0 items-center justify-between border-b border-line px-5">
        <div className="min-w-0 flex items-center gap-1">
          <h2 className="truncate font-serif text-lg font-semibold">{room.title ?? room.id}</h2>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={onToggleRoster}
            aria-label={t('collab.rooms.members')}
            aria-expanded={rosterOpen}
          >
            {rosterOpen ? <PanelRightClose size={16} /> : <PanelRightOpen size={16} />}
          </Button>
          {room.unreadCount > 0 && lastMessage ? (
            <Button type="button" variant="ghost" size="sm" onClick={() => void markRead(room.id, lastMessage.sequence)}>
              {t('collab.rooms.markRead')}
            </Button>
          ) : null}
        </div>
      </header>
      {held ? (
        <div className="border-b border-amber-300 bg-amber-50 px-5 py-2 text-xs text-amber-900" role="status">
          {t('collab.rooms.held', { agent: room.members.find((member) => member.id === held.agentId)?.displayName ?? held.agentId })}
        </div>
      ) : null}
      <div ref={viewportRef} className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        {window?.hasOlder ? (
          <div className="mb-4 text-center"><Button type="button" variant="ghost" size="sm" onClick={() => void loadOlder(room.id)}><ArrowUp size={15} />{t('collab.rooms.older')}</Button></div>
        ) : null}
        <div className="grid">
          {window?.messages.map((message, index) => {
            if (message.kind === 'system') {
              return <SystemNotice key={message.id} message={message} first={index === 0} onOpenBoard={() => navigate('boards')} />
            }
            const previous = window.messages[index - 1]
            const grouped = previous !== undefined
              && previous.kind === 'normal'
              && previous.authorId === message.authorId
            const own = message.authorId === 'user'
            const author = room.members.find((member) => member.id === message.authorId)
            const name = own ? t('collab.rooms.user') : author?.displayName ?? message.authorId
            const bubble = own
              ? `bg-clay-soft ${grouped ? '' : 'rounded-tr-md'}`
              : `bg-surface ${grouped ? '' : 'rounded-tl-md'}`
            return (
              <article
                key={message.id}
                data-message-sequence={message.sequence}
                className={`flex gap-2.5 ${own ? 'flex-row-reverse' : ''} ${grouped ? 'mt-1' : index > 0 ? 'mt-4' : ''}`}
              >
                {grouped ? (
                  <span className="size-8 shrink-0" aria-hidden="true" />
                ) : (
                  <span className={`grid size-8 shrink-0 place-items-center rounded-full ${own ? 'bg-ink/10 text-ink' : 'bg-clay/10 text-clay'}`}>
                    {own ? <UserRound size={16} /> : <Bot size={16} />}
                  </span>
                )}
                <div className={`grid min-w-0 max-w-[min(72%,40rem)] gap-1 ${own ? 'justify-items-end' : ''}`}>
                  {grouped ? null : (
                    <header className={`flex items-baseline gap-2 ${own ? 'flex-row-reverse' : ''}`}>
                      <strong className="text-xs font-semibold text-ink-soft">{name}</strong>
                      <time className="text-[11px] text-ink-faint">{formatBeijingDateTime(message.createdAt)}</time>
                    </header>
                  )}
                  <div className={`w-fit min-w-0 max-w-full rounded-2xl px-3.5 py-2 ${bubble}`}>
                    <MarkdownRenderer content={message.body} variant="compact" />
                  </div>
                </div>
              </article>
            )
          })}
        </div>
        {window?.hasNewer ? (
          <div className="mt-4 text-center"><Button type="button" variant="ghost" size="sm" onClick={() => void loadNewer(room.id)}><ArrowDown size={15} />{t('collab.rooms.newer')}</Button></div>
        ) : null}
        {window?.loading ? <p className="py-4 text-center text-sm text-ink-faint">{t('collab.rooms.loading')}</p> : null}
        {!window?.loading && window?.messages.length === 0 ? <p className="py-16 text-center text-sm text-ink-faint">{t('collab.rooms.noMessages')}</p> : null}
        {window?.error ? <p className="py-2 text-sm text-red-600">{window.error}</p> : null}
      </div>
      <form className="shrink-0 border-t border-line p-4" onSubmit={submit}>
        {enabledAgents.length > 0 ? (
          <div className="mb-2 flex flex-wrap gap-1">
            {enabledAgents.map((agent) => (
              <Button key={agent.id} type="button" variant="ghost" size="sm" className="h-7 rounded-lg px-2 text-xs" onClick={() => setDraft((value) => `${value}${value && !value.endsWith(' ') ? ' ' : ''}@${agent.id} `)}>
                <AtSign size={12} />{agent.displayName}
              </Button>
            ))}
          </div>
        ) : null}
        <div className="flex items-end gap-2">
          <Textarea value={draft} rows={2} placeholder={t('collab.rooms.messagePlaceholder')} aria-label={t('collab.rooms.messagePlaceholder')} onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey) {
              event.preventDefault()
              event.currentTarget.form?.requestSubmit()
            }
          }} />
          <Button type="submit" size="icon" className="size-10 shrink-0 rounded-xl" aria-label={t('collab.rooms.send')}><Send size={17} /></Button>
        </div>
      </form>
    </section>
  )
}

function SystemNotice({ message, first, onOpenBoard }: {
  message: CollabMessage
  first: boolean
  onOpenBoard: () => void
}) {
  const { t } = useTranslation()
  const cardId = systemCardId(message)
  const proactive = proactiveMessageDetails(message)
  const top = first ? '' : 'mt-4'
  if (proactive) {
    return (
      <div data-message-sequence={message.sequence} className={`grid justify-items-center gap-1 text-center ${top}`}>
        <span data-proactive-trigger={proactive.trigger} className="flex items-center gap-1 rounded-full bg-ink/5 px-2.5 py-0.5 text-xs font-medium text-ink-faint">
          <Sparkles size={12} />{t(`collab.rooms.${proactive.trigger}Wake`)}
        </span>
        <span className="text-xs text-ink-faint">{proactive.reason}</span>
      </div>
    )
  }
  return (
    <div data-message-sequence={message.sequence} className={`flex justify-center ${top}`}>
      {cardId ? (
        <button type="button" data-system-card-id={cardId} className="rounded-full bg-ink/5 px-3 py-1 text-xs font-medium text-clay hover:underline" onClick={onOpenBoard}>
          {message.body}
        </button>
      ) : (
        <span className="rounded-full bg-ink/5 px-3 py-1 text-xs text-ink-faint">{message.body}</span>
      )}
    </div>
  )
}

export function systemCardId(message: CollabMessage): string | null {
  return message.kind === 'system' && typeof message.systemPayload?.cardId === 'string'
    ? message.systemPayload.cardId
    : null
}

export function proactiveMessageDetails(message: CollabMessage): {
  trigger: 'agenda' | 'scanner'
  reason: string
} | null {
  if (message.kind !== 'system' || message.systemPayload?.type !== 'proactive_wake') return null
  const trigger = message.systemPayload.trigger
  const reason = message.systemPayload.reason
  return (trigger === 'agenda' || trigger === 'scanner') && typeof reason === 'string'
    ? { trigger, reason }
    : null
}
