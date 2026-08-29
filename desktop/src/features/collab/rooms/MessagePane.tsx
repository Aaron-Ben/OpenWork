import { Bot, Send, UserRound } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabRoom } from '@/bridge/collab'
import { MarkdownRenderer } from '@/components/markdown/MarkdownRenderer'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { useMessageStore } from './messageStore'

export function MessagePane({ room }: { room: CollabRoom }) {
  const { t } = useTranslation()
  const messages = useMessageStore((state) => state.byRoom[room.id])
  const open = useMessageStore((state) => state.open)
  const send = useMessageStore((state) => state.send)
  const [draft, setDraft] = useState('')

  useEffect(() => {
    void open(room.id)
    const timer = globalThis.setInterval(() => void open(room.id), 2_000)
    return () => globalThis.clearInterval(timer)
  }, [open, room.id])

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    const body = draft.trim()
    if (!body) return
    setDraft('')
    await send(room.id, body)
  }

  return (
    <section className="flex min-w-0 flex-1 flex-col bg-paper">
      <header data-tauri-drag-region="deep" className="flex h-12 shrink-0 items-center border-b border-line px-5">
        <h2 className="truncate font-serif text-lg font-semibold">{room.title ?? room.id}</h2>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        <div className="grid gap-4">
          {messages?.messages.map((message) => {
            const own = message.authorId === 'user'
            return (
              <article key={message.id} data-message-sequence={message.sequence} className={`flex gap-2.5 ${own ? 'flex-row-reverse' : ''}`}>
                <span className={`grid size-8 shrink-0 place-items-center rounded-full ${own ? 'bg-ink/10 text-ink' : 'bg-clay/10 text-clay'}`}>
                  {own ? <UserRound size={16} /> : <Bot size={16} />}
                </span>
                <div className={`grid min-w-0 max-w-[min(72%,40rem)] gap-1 ${own ? 'justify-items-end' : ''}`}>
                  <strong className="text-xs font-semibold text-ink-soft">{own ? t('collab.rooms.user') : message.authorId}</strong>
                  <div className={`w-fit min-w-0 max-w-full rounded-2xl px-3.5 py-2 ${own ? 'bg-clay-soft' : 'bg-surface'}`}>
                    <MarkdownRenderer content={message.body} variant="compact" />
                  </div>
                </div>
              </article>
            )
          })}
        </div>
        {messages?.loading ? <p className="py-4 text-center text-sm text-ink-faint">{t('collab.rooms.loading')}</p> : null}
        {!messages?.loading && messages?.messages.length === 0 ? <p className="py-16 text-center text-sm text-ink-faint">{t('collab.rooms.noMessages')}</p> : null}
        {messages?.error ? <p className="py-2 text-sm text-red-600">{messages.error}</p> : null}
      </div>
      <form className="shrink-0 border-t border-line p-4" onSubmit={submit}>
        <div className="flex items-end gap-2">
          <Textarea value={draft} rows={2} placeholder={t('collab.rooms.messagePlaceholder')} onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => {
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
