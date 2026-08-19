import { Bot, ShieldAlert, UserRoundPlus } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabRoomSummary } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { ApprovalCard } from '@/features/collab/permissions/ApprovalCard'
import { usePermissionStore } from '@/features/collab/permissions/permissionStore'
import { useRoomStore } from './roomStore'

export function Roster({ room, agents }: { room: CollabRoomSummary; agents: CollabAgent[] }) {
  const { t } = useTranslation()
  const pending = usePermissionStore((state) => state.pending)
  const addMember = useRoomStore((state) => state.addMember)
  const memberIds = new Set(room.members.map((member) => member.id))
  const available = agents.filter((agent) => agent.enabled && !memberIds.has(agent.id))
  const roster = room.members.filter((member) => member.kind === 'agent' && member.enabled)

  return (
    <aside className="flex h-full w-72 shrink-0 flex-col overflow-y-auto border-l border-line bg-paper-hover p-3">
      {pending.length > 0 ? (
        <section className="mb-5 grid gap-2">
          <h2 className="flex items-center gap-2 px-1 text-sm font-semibold text-red-700"><ShieldAlert size={16} />{t('collab.approvals.title')}</h2>
          {pending.map((permission) => (
            <ApprovalCard
              key={permission.id}
              permission={permission}
              agentName={agents.find((agent) => agent.id === permission.agentId)?.displayName ?? permission.agentId ?? 'Agent'}
            />
          ))}
        </section>
      ) : null}
      <h2 className="px-1 pb-2 text-sm font-semibold">{t('collab.rooms.members')}</h2>
      <div className="grid gap-1">
        {roster.map((member) => {
          const waiting = pending.some((permission) => permission.agentId === member.id)
          return (
            <div key={member.id} className="flex items-center gap-3 rounded-xl px-3 py-2">
              <span className={`grid size-8 place-items-center rounded-full ${waiting ? 'bg-red-100 text-red-700' : 'bg-clay/10 text-clay'}`}><Bot size={16} /></span>
              <span className="min-w-0 flex-1">
                <strong className="block truncate text-sm">{member.displayName}</strong>
                <span className={`block text-xs ${waiting ? 'text-red-700' : 'text-ink-faint'}`}>{waiting ? t('collab.agents.waitingApproval') : t('collab.agents.idle')}</span>
              </span>
            </div>
          )
        })}
      </div>
      {available.length > 0 ? (
        <div className="mt-4 border-t border-line pt-3">
          <p className="px-1 pb-2 text-xs font-medium text-ink-faint">{t('collab.rooms.addMember')}</p>
          {available.map((agent) => (
            <Button key={agent.id} type="button" variant="ghost" size="sm" className="w-full justify-start" onClick={() => void addMember(room.id, agent.id)}>
              <UserRoundPlus size={15} />{agent.displayName}
            </Button>
          ))}
        </div>
      ) : null}
    </aside>
  )
}
