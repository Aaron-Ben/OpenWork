import { Hash, Plus, Users } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabRoom } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useRoomStore } from './roomStore'

export function RoomList({ rooms, agents, activeRoomId, onSelect }: {
  rooms: CollabRoom[]
  agents: CollabAgent[]
  activeRoomId: string | null
  onSelect: (roomId: string) => void
}) {
  const { t } = useTranslation()
  const createGroup = useRoomStore((state) => state.createGroup)
  const error = useRoomStore((state) => state.error)
  const [creating, setCreating] = useState(false)
  const [title, setTitle] = useState('')
  const [selected, setSelected] = useState<string[]>([])

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    if (!title.trim() || selected.length < 2) return
    const room = await createGroup(title.trim(), selected)
    if (!room) return
    setTitle('')
    setSelected([])
    setCreating(false)
    onSelect(room.id)
  }

  function toggle(agentId: string) {
    setSelected((current) => current.includes(agentId)
      ? current.filter((id) => id !== agentId)
      : [...current, agentId])
  }

  return (
    <aside className="flex h-full w-64 min-w-52 max-w-96 shrink-0 resize-x flex-col overflow-auto border-r border-line bg-paper-hover">
      <header className="flex h-12 shrink-0 items-center justify-between px-4">
        <h1 className="font-serif text-lg font-semibold">{t('collab.rooms.title')}</h1>
        <Button type="button" variant="ghost" size="icon" className="size-8" disabled={agents.filter((agent) => agent.enabled).length < 2} aria-label={t('collab.rooms.createGroup')} onClick={() => setCreating(true)}>
          <Plus size={16} />
        </Button>
      </header>
      <nav className="grid gap-1 p-2">
        {rooms.map((room) => (
          <Button key={room.id} type="button" variant="ghost" className={`h-auto min-h-10 justify-start rounded-xl px-3 py-2 ${activeRoomId === room.id ? 'bg-paper shadow-sm' : ''}`} onClick={() => onSelect(room.id)}>
            <Hash size={16} className="shrink-0 text-ink-faint" />
            <span className="truncate">{room.title ?? room.id}</span>
          </Button>
        ))}
        {rooms.length === 0 ? <p className="px-3 py-8 text-center text-sm text-ink-faint">{t('collab.rooms.empty')}</p> : null}
      </nav>
      {creating ? (
        <div className="fixed inset-0 z-30 grid place-items-center bg-black/30 p-6" role="dialog" aria-modal="true">
          <form className="grid w-full max-w-md gap-4 rounded-3xl bg-paper p-6 shadow-xl" onSubmit={submit}>
            <div className="flex items-center gap-2">
              <Users size={20} className="text-clay" />
              <h2 className="font-serif text-xl font-semibold">{t('collab.rooms.createGroup')}</h2>
            </div>
            <label className="grid gap-1 text-xs font-medium text-ink-muted">
              <span>{t('collab.rooms.groupName')}</span>
              <Input required maxLength={120} value={title} onChange={(event) => setTitle(event.target.value)} />
            </label>
            <fieldset className="grid gap-2">
              <legend className="mb-1 text-xs font-medium text-ink-muted">{t('collab.rooms.selectMembers')}</legend>
              {agents.filter((agent) => agent.enabled).map((agent) => (
                <label key={agent.id} className="flex items-center gap-3 rounded-xl border border-line px-3 py-2 text-sm">
                  <input type="checkbox" checked={selected.includes(agent.id)} onChange={() => toggle(agent.id)} />
                  <span className="min-w-0 flex-1 truncate">{agent.displayName}</span>
                  <span className="text-xs text-ink-faint">@{agent.id}</span>
                </label>
              ))}
              <p className="text-xs text-ink-faint">{t('collab.rooms.minimumMembers')}</p>
            </fieldset>
            {error ? <p className="text-sm text-red-600">{error}</p> : null}
            <div className="flex justify-end gap-2">
              <Button type="button" variant="ghost" onClick={() => setCreating(false)}>{t('common.cancel')}</Button>
              <Button type="submit" variant="accent" disabled={!title.trim() || selected.length < 2}>{t('collab.rooms.createGroup')}</Button>
            </div>
          </form>
        </div>
      ) : null}
    </aside>
  )
}
