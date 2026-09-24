import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'

import type { CollabAgent, CollabRoomSummary } from '@/bridge/collab'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { ParticipantAvatar, RoomHive } from '@/features/collab/components/ParticipantAvatar'
import { beijingDayStamp } from '@/lib/dateTime'
import { cn } from '@/lib/utils'
import { roomWorkingLabel } from './roomWorking'
import { WorkingDots } from './WorkingDots'

/** 房间列表的一行（collaboration-desktop.md §7.1）；右键菜单可置顶，群组还可管理成员。 */
export function RoomListRow({ room, agents, names, active, now, onSelect, onPin, onManage }: {
  room: CollabRoomSummary
  agents: CollabAgent[]
  names: ReadonlyMap<string, string>
  active: boolean
  now: number
  onSelect: () => void
  onPin: (pinned: boolean) => void
  onManage: () => void
}) {
  const { t } = useTranslation()
  const [menuOpen, setMenuOpen] = useState(false)
  const working = roomWorkingLabel(agents, room.id)
  const stamp = room.lastMessageAt ? beijingDayStamp(room.lastMessageAt, now) : null
  const title = room.title ?? room.id
  const otherId = room.memberIds.find((id) => id !== 'local-user') ?? room.id

  return (
    <div className="relative" onContextMenu={(event) => { event.preventDefault(); setMenuOpen(true) }}>
      <button
        type="button"
        className={cn('flex w-full items-center gap-3 rounded-xl px-2.5 py-2 text-left', active ? 'bg-paper shadow-sm' : 'hover:bg-paper/60')}
        onClick={onSelect}
      >
        {room.kind === 'group'
          ? <RoomHive memberIds={room.memberIds} names={names} you={t('collab.rooms.user')} />
          : <ParticipantAvatar participantId={otherId} name={names.get(otherId) ?? title} size={38} />}
        <span className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className={cn('truncate text-sm', room.unreadCount > 0 ? 'font-semibold' : 'font-medium')}>{title}</span>
          {working ? (
            <span className="flex items-center gap-1.5 truncate text-xs text-clay-ink"><WorkingDots />{t(working.key, working.values)}</span>
          ) : room.lastMessage ? (
            <span className={cn('truncate text-xs', room.unreadCount > 0 ? 'text-ink-soft' : 'text-ink-soft')}>
              {t('collab.rooms.lastMessage', { author: room.lastMessage.authorName, body: room.lastMessage.body })}
            </span>
          ) : null}
        </span>
        <span className="flex shrink-0 flex-col items-end gap-1">
          {stamp ? <span className="text-[11px] tabular-nums text-ink-soft">{stampText(stamp, t)}</span> : null}
          {room.unreadCount > 0 ? (
            <span className="grid h-[18px] min-w-[18px] place-items-center rounded-full bg-clay px-1.5 text-[11px] font-bold text-ink">{room.unreadCount}</span>
          ) : null}
        </span>
      </button>
      <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
        <DropdownMenuTrigger asChild>
          <span aria-hidden="true" tabIndex={-1} className="pointer-events-none absolute right-2 top-1/2 size-0" />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" aria-label={t('collab.rooms.roomActions')}>
          <DropdownMenuItem onSelect={() => onPin(!room.pinned)}>{room.pinned ? t('collab.rooms.unpin') : t('collab.rooms.pin')}</DropdownMenuItem>
          {room.kind === 'group' ? <DropdownMenuItem onSelect={onManage}>{t('collab.rooms.manageMembers')}</DropdownMenuItem> : null}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  )
}

function stampText(stamp: NonNullable<ReturnType<typeof beijingDayStamp>>, t: TFunction): string {
  switch (stamp.kind) {
    case 'today': return stamp.clock
    case 'yesterday': return t('collab.rooms.yesterday')
    case 'date': return t('collab.rooms.monthDay', { month: stamp.month, day: stamp.day })
    default: {
      const unreachable: never = stamp
      return unreachable
    }
  }
}
