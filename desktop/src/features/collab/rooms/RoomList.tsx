import { Hash } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { CollabRoom } from '@/bridge/collab'
import { Button } from '@/components/ui/button'

export function RoomList({ rooms, activeRoomId, onSelect }: {
  rooms: CollabRoom[]
  activeRoomId: string | null
  onSelect: (roomId: string) => void
}) {
  const { t } = useTranslation()
  return (
    <aside className="flex h-full w-64 min-w-52 max-w-96 shrink-0 resize-x flex-col overflow-auto border-r border-line bg-paper-hover">
      <header className="flex h-12 shrink-0 items-center px-4">
        <h1 className="font-serif text-lg font-semibold">{t('collab.rooms.title')}</h1>
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
    </aside>
  )
}
