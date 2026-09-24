import { UserMinus, UserPlus, Users } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useMessageStore } from './messageStore'
import { membersForRoom, useRoomStore } from './roomStore'

/** 群组成员管理：添加、移除 Agent；用户始终由 Server 管理（collaboration-desktop.md §4.2）。 */
export function ManageMembersDialog({ roomId, agents, onClose }: {
  roomId: string
  agents: CollabAgent[]
  onClose: () => void
}) {
  const { t } = useTranslation()
  const members = useRoomStore((state) => membersForRoom(state, roomId))
  const fetchMembers = useRoomStore((state) => state.fetchMembers)
  const addMember = useRoomStore((state) => state.addMember)
  const removeMember = useRoomStore((state) => state.removeMember)
  const fetchRooms = useRoomStore((state) => state.fetchAll)
  const roomError = useRoomStore((state) => state.error)
  const open = useMessageStore((state) => state.open)
  const [agentToAdd, setAgentToAdd] = useState('')
  const available = agents.filter((agent) => agent.archivedAt === null && !members.some((member) => member.id === agent.id))

  useEffect(() => { void fetchMembers(roomId) }, [fetchMembers, roomId])

  async function refresh() {
    await Promise.all([open(roomId), fetchRooms()])
  }

  async function invite() {
    if (!agentToAdd) return
    await addMember(roomId, agentToAdd)
    setAgentToAdd('')
    await refresh()
  }

  async function remove(agentId: string) {
    await removeMember(roomId, agentId)
    await refresh()
  }

  return (
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
                <Button type="button" variant="ghost" size="icon" className="size-8 text-status-danger-ink" aria-label={t('collab.rooms.removeMember', { name: member.displayName })} onClick={() => void remove(member.id)}>
                  <UserMinus size={15} />
                </Button>
              ) : null}
            </div>
          ))}
        </div>
        {available.length > 0 ? (
          <div className="flex gap-2">
            <Select value={agentToAdd || undefined} onValueChange={setAgentToAdd}>
              <SelectTrigger className="h-9 min-w-0 flex-1 border border-line bg-paper px-3 text-sm">
                <SelectValue placeholder={t('collab.rooms.selectAgent')} />
              </SelectTrigger>
              <SelectContent sideOffset={5}>
                {available.map((agent) => <SelectItem key={agent.id} value={agent.id}>{agent.displayName} (@{agent.id})</SelectItem>)}
              </SelectContent>
            </Select>
            <Button type="button" variant="outline" disabled={!agentToAdd} onClick={() => void invite()}>
              <UserPlus size={15} />{t('collab.rooms.addMember')}
            </Button>
          </div>
        ) : null}
        {roomError ? <p className="text-sm text-status-danger-ink">{roomError}</p> : null}
        <div className="flex justify-end">
          <Button type="button" variant="accent" onClick={onClose}>{t('common.close')}</Button>
        </div>
      </section>
    </div>
  )
}
