import { Bell, BellOff, Hash, Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabRoomSummary } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useRoomStore } from './roomStore'

interface RoomListProps {
  rooms: CollabRoomSummary[]
  activeRoomId: string | null
  onSelect: (roomId: string) => void
  onCreate: (title: string) => Promise<void>
}

export function RoomList({ rooms, activeRoomId, onSelect, onCreate }: RoomListProps) {
  const { t } = useTranslation()
  const [creating, setCreating] = useState(false)
  const [title, setTitle] = useState('')
  const setMuted = useRoomStore((state) => state.setMuted)

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    if (!title.trim()) return
    await onCreate(title.trim())
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
          <Input required value={title} placeholder={t('collab.rooms.name')} aria-label={t('collab.rooms.name')} onChange={(event) => setTitle(event.target.value)} />
          <Button type="submit" size="sm">{t('collab.rooms.create')}</Button>
        </form>
      ) : null}
      <nav className="grid gap-1 p-2">
        {rooms.map((room) => (
          <div key={room.id} className={`flex items-center rounded-xl ${activeRoomId === room.id ? 'bg-paper shadow-sm' : ''}`}>
            <Button
              type="button"
              variant="ghost"
              className="h-auto min-h-10 min-w-0 flex-1 justify-start rounded-xl px-3 py-2"
              onClick={() => onSelect(room.id)}
            >
              <Hash size={16} className="shrink-0 text-ink-faint" />
              <span className="min-w-0 flex-1 truncate text-left">{room.title ?? room.id}</span>
              {room.unreadCount > 0 ? (
                <span className="rounded-full bg-clay px-1.5 text-[11px] font-semibold text-white">{room.unreadCount}</span>
              ) : null}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="mr-1 size-8 shrink-0 rounded-lg"
              aria-label={t(room.muted ? 'collab.rooms.unmuteRoom' : 'collab.rooms.muteRoom')}
              title={t(room.muted ? 'collab.rooms.unmuteRoom' : 'collab.rooms.muteRoom')}
              onClick={() => void setMuted(room.id, 'user', !room.muted)}
            >
              {room.muted ? <BellOff size={15} /> : <Bell size={15} />}
            </Button>
          </div>
        ))}
        {rooms.length === 0 ? <p className="px-3 py-8 text-center text-sm text-ink-faint">{t('collab.rooms.empty')}</p> : null}
      </nav>
    </aside>
  )
}
