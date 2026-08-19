import { ArrowDown, ArrowUp, AtSign, Send } from 'lucide-react'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabRoomSummary } from '@/bridge/collab'
import { MarkdownRenderer } from '@/components/markdown/MarkdownRenderer'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { formatBeijingDateTime } from '@/lib/dateTime'
import { useCoordinationStore } from '@/features/collab/coordinationStore'
import { useMessageStore } from './messageStore'
import { useRoomStore } from './roomStore'

export function MessagePane({ room }: { room: CollabRoomSummary }) {
  const { t } = useTranslation()
  const window = useMessageStore((state) => state.byRoom[room.id])
  const open = useMessageStore((state) => state.open)
  const loadOlder = useMessageStore((state) => state.loadOlder)
  const loadNewer = useMessageStore((state) => state.loadNewer)
  const send = useMessageStore((state) => state.send)
  const markRead = useRoomStore((state) => state.markRead)
  const held = useCoordinationStore((state) => state.heldByRoom[room.id])
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
        <div className="min-w-0">
          <h2 className="truncate font-serif text-lg font-semibold">{room.title ?? room.id}</h2>
        </div>
        {room.unreadCount > 0 && lastMessage ? (
          <Button type="button" variant="ghost" size="sm" onClick={() => void markRead(room.id, lastMessage.sequence)}>
            {t('collab.rooms.markRead')}
          </Button>
        ) : null}
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
        <div className="grid gap-4">
          {window?.messages.map((message) => {
            const author = room.members.find((member) => member.id === message.authorId)
            return (
              <article key={message.id} data-message-sequence={message.sequence} className="rounded-2xl border border-line bg-paper-hover px-4 py-3">
                <header className="mb-2 flex items-baseline justify-between gap-4">
                  <strong className="text-sm">{message.authorId === 'user' ? t('collab.rooms.user') : author?.displayName ?? message.authorId}</strong>
                  <time className="text-[11px] text-ink-faint">{formatBeijingDateTime(message.createdAt)}</time>
                </header>
                <MarkdownRenderer content={message.body} variant="compact" />
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
