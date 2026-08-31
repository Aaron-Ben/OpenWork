import { AlertTriangle, Bot, LoaderCircle, Send, UserMinus, UserPlus, UserRound, Users } from 'lucide-react'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabRoom } from '@/bridge/collab'
import { MarkdownRenderer } from '@/components/markdown/MarkdownRenderer'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { ScrollToLatestButton } from '@/features/collab/components/ScrollToLatestButton'
import { useMessageStore } from './messageStore'
import { roomRunState } from './roomRunState'
import { membersForRoom, useRoomStore } from './roomStore'

export function MessagePane({ room, agents }: { room: CollabRoom; agents: CollabAgent[] }) {
  const { t } = useTranslation()
  const messages = useMessageStore((state) => state.byRoom[room.id])
  const open = useMessageStore((state) => state.open)
  const send = useMessageStore((state) => state.send)
  const members = useRoomStore((state) => membersForRoom(state, room.id))
  const fetchMembers = useRoomStore((state) => state.fetchMembers)
  const addMember = useRoomStore((state) => state.addMember)
  const removeMember = useRoomStore((state) => state.removeMember)
  const roomError = useRoomStore((state) => state.error)
  const [draft, setDraft] = useState('')
  const [managing, setManaging] = useState(false)
  const [agentToAdd, setAgentToAdd] = useState('')
  const [atBottom, setAtBottom] = useState(true)
  const scrollAreaRef = useRef<HTMLDivElement>(null)
  const runState = roomRunState(messages?.runs ?? [], room.id)
  const runAgents = runState?.agentIds.map((id) => `@${id}`).join(', ') ?? ''
  const availableAgents = agents.filter((agent) => agent.archivedAt === null && !members.some((member) => member.id === agent.id))

  useEffect(() => {
    setAtBottom(true)
    void open(room.id)
  }, [open, room.id])

  const latestSequence = messages?.messages[messages.messages.length - 1]?.sequence ?? 0
  useLayoutEffect(() => {
    if (!atBottom) return
    const frame = globalThis.requestAnimationFrame(() => scrollToLatest('auto'))
    return () => globalThis.cancelAnimationFrame(frame)
  }, [atBottom, latestSequence, room.id])

  function scrollToLatest(behavior: ScrollBehavior = 'smooth') {
    const element = scrollAreaRef.current
    if (!element) return
    element.scrollTo({ top: element.scrollHeight, behavior })
    setAtBottom(true)
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    const body = draft.trim()
    if (!body) return
    setDraft('')
    setAtBottom(true)
    await send(room.id, body)
    scrollToLatest()
  }

  async function openMembers() {
    setManaging(true)
    setAgentToAdd('')
    await fetchMembers(room.id)
  }

  async function invite() {
    if (!agentToAdd) return
    await addMember(room.id, agentToAdd)
    setAgentToAdd('')
    await open(room.id)
  }

  async function remove(agentId: string) {
    await removeMember(room.id, agentId)
    await open(room.id)
  }

  function runLabel(): string | null {
    if (!runState) return null
    if (runState.kind === 'thinking') return t('collab.rooms.thinking', { agents: runAgents })
    if (runState.kind === 'retrying') return t('collab.rooms.retrying', { agents: runAgents })
    if (runState.kind === 'rate_limited') return t('collab.rooms.rateLimited', { agents: runAgents })
    return t('collab.rooms.runFailed', { agents: runAgents, message: runState.message ?? t('collab.rooms.unknownFailure') })
  }

  const statusLabel = runLabel()

  return (
    <section className="flex min-w-0 flex-1 flex-col bg-paper">
      <header data-tauri-drag-region="deep" className="flex h-12 shrink-0 items-center justify-between gap-3 border-b border-line px-5">
        <h2 className="truncate font-serif text-lg font-semibold">{room.title ?? room.id}</h2>
        {room.kind === 'group' ? (
          <Button type="button" variant="ghost" size="sm" onClick={() => void openMembers()}>
            <Users size={16} />{t('collab.rooms.manageMembers')}
          </Button>
        ) : null}
      </header>
      {statusLabel ? (
        <div className={`flex items-center gap-2 border-b border-line px-5 py-2 text-xs ${runState?.kind === 'failed' || runState?.kind === 'rate_limited' ? 'bg-red-50 text-red-700' : 'bg-clay/5 text-ink-muted'}`}>
          {runState?.kind === 'failed' || runState?.kind === 'rate_limited'
            ? <AlertTriangle size={14} />
            : <LoaderCircle size={14} className="animate-spin" />}
          <span>{statusLabel}</span>
        </div>
      ) : null}
      <div className="relative min-h-0 flex-1">
        <div
          ref={scrollAreaRef}
          className="h-full overflow-y-auto px-5 py-4"
          onScroll={(event) => {
            const element = event.currentTarget
            setAtBottom(element.scrollHeight - element.scrollTop - element.clientHeight < 72)
          }}
        >
          <div className="grid gap-4">
            {messages?.messages.map((message) => {
              const own = message.authorId === 'local-user'
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
        <ScrollToLatestButton visible={!atBottom} label={t('collab.rooms.scrollToLatest')} onClick={() => scrollToLatest()} />
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
      {managing ? (
        <div className="fixed inset-0 z-30 grid place-items-center bg-black/30 p-6" role="dialog" aria-modal="true">
          <section className="grid w-full max-w-md gap-4 rounded-3xl bg-paper p-6 shadow-xl">
            <div className="flex items-center gap-2">
              <Users size={20} className="text-clay" />
              <h2 className="font-serif text-xl font-semibold">{t('collab.rooms.manageMembers')}</h2>
            </div>
            <div className="grid gap-2">
              {members.map((member) => (
                <div key={member.id} className="flex items-center gap-3 rounded-xl border border-line px-3 py-2 text-sm">
                  <span className="min-w-0 flex-1 truncate">{member.displayName}</span>
                  <span className="text-xs text-ink-faint">@{member.id}</span>
                  {member.kind === 'agent' ? (
                    <Button type="button" variant="ghost" size="icon" className="size-8 text-red-600" aria-label={t('collab.rooms.removeMember', { name: member.displayName })} onClick={() => void remove(member.id)}>
                      <UserMinus size={15} />
                    </Button>
                  ) : null}
                </div>
              ))}
            </div>
            {availableAgents.length > 0 ? (
              <div className="flex gap-2">
                <Select value={agentToAdd || undefined} onValueChange={setAgentToAdd}>
                  <SelectTrigger className="h-9 min-w-0 flex-1 border border-line bg-paper px-3 text-sm">
                    <SelectValue placeholder={t('collab.rooms.selectAgent')} />
                  </SelectTrigger>
                  <SelectContent sideOffset={5}>
                    {availableAgents.map((agent) => <SelectItem key={agent.id} value={agent.id}>{agent.displayName} (@{agent.id})</SelectItem>)}
                  </SelectContent>
                </Select>
                <Button type="button" variant="outline" disabled={!agentToAdd} onClick={() => void invite()}>
                  <UserPlus size={15} />{t('collab.rooms.addMember')}
                </Button>
              </div>
            ) : null}
            {roomError ? <p className="text-sm text-red-600">{roomError}</p> : null}
            <div className="flex justify-end">
              <Button type="button" variant="accent" onClick={() => setManaging(false)}>{t('common.close')}</Button>
            </div>
          </section>
        </div>
      ) : null}
    </section>
  )
}
