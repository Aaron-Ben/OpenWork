import { Hash, Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabRoomSummary } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

interface RoomListProps {
  rooms: CollabRoomSummary[]
  activeRoomId: string | null
  onSelect: (roomId: string) => void
  onCreate: (id: string, title: string) => Promise<void>
}

export function RoomList({ rooms, activeRoomId, onSelect, onCreate }: RoomListProps) {
  const { t } = useTranslation()
  const [creating, setCreating] = useState(false)
  const [id, setId] = useState('')
  const [title, setTitle] = useState('')

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    if (!id.trim() || !title.trim()) return
    await onCreate(id.trim(), title.trim())
    setId('')
    setTitle('')
    setCreating(false)
  }

  return (
    <aside className="flex h-full w-64 min-w-52 max-w-96 shrink-0 resize-x flex-col overflow-auto border-r border-line bg-paper-hover">
      <header className="flex h-12 shrink-0 items-center justify-between px-4">
        <h1 className="font-serif text-lg font-semibold">{t('collab.rooms.title')}</h1>
        <Button type="button" variant="ghost" size="icon" className="size-8 rounded-xl" aria-label={t('collab.rooms.create')} onClick={() => setCreating((value) => !value)}>
          <Plus size={17} />
        </Button>
      </header>
      {creating ? (
        <form className="grid gap-2 border-y border-line p-3" onSubmit={submit}>
          <Input value={id} pattern="[a-z][a-z0-9_]{0,47}" placeholder={t('collab.rooms.id')} aria-label={t('collab.rooms.id')} onChange={(event) => setId(event.target.value)} />
          <Input value={title} placeholder={t('collab.rooms.name')} aria-label={t('collab.rooms.name')} onChange={(event) => setTitle(event.target.value)} />
          <Button type="submit" size="sm">{t('collab.rooms.create')}</Button>
        </form>
      ) : null}
      <nav className="grid gap-1 p-2">
        {rooms.map((room) => (
          <Button
            key={room.id}
            type="button"
            variant="ghost"
            className={`h-auto min-h-10 justify-start rounded-xl px-3 py-2 ${activeRoomId === room.id ? 'bg-paper shadow-sm' : ''}`}
            onClick={() => onSelect(room.id)}
          >
            <Hash size={16} className="shrink-0 text-ink-faint" />
            <span className="min-w-0 flex-1 truncate text-left">{room.title ?? room.id}</span>
            {room.unreadCount > 0 ? (
              <span className="rounded-full bg-clay px-1.5 text-[11px] font-semibold text-white">{room.unreadCount}</span>
            ) : null}
          </Button>
        ))}
        {rooms.length === 0 ? <p className="px-3 py-8 text-center text-sm text-ink-faint">{t('collab.rooms.empty')}</p> : null}
      </nav>
    </aside>
  )
}
