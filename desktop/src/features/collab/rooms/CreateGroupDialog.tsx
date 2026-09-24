import { Users } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useRoomStore } from './roomStore'

/** 新建群组：名称与至少两位 Agent。创建成功后回调房间 id。 */
export function CreateGroupDialog({ agents, onCreated, onClose }: {
  agents: CollabAgent[]
  onCreated: (roomId: string) => void
  onClose: () => void
}) {
  const { t } = useTranslation()
  const createGroup = useRoomStore((state) => state.createGroup)
  const error = useRoomStore((state) => state.error)
  const [title, setTitle] = useState('')
  const [selected, setSelected] = useState<string[]>([])
  const active = agents.filter((agent) => agent.archivedAt === null)

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    if (!title.trim() || selected.length < 2) return
    const room = await createGroup(title.trim(), selected)
    if (room) onCreated(room.id)
  }

  function toggle(agentId: string) {
    setSelected((current) => current.includes(agentId)
      ? current.filter((id) => id !== agentId)
      : [...current, agentId])
  }

  return (
    <div className="fixed inset-0 z-30 grid place-items-center bg-black/30 p-6" role="dialog" aria-modal="true">
      <form className="grid w-full max-w-md gap-4 rounded-3xl bg-paper p-6 shadow-xl" onSubmit={submit}>
        <div className="flex items-center gap-2">
          <Users size={20} className="text-clay-ink" />
          <h2 className="font-serif text-xl font-semibold">{t('collab.rooms.createGroup')}</h2>
        </div>
        <label className="grid gap-1 text-xs font-medium text-ink-soft">
          <span>{t('collab.rooms.groupName')}</span>
          <Input required maxLength={120} value={title} onChange={(event) => setTitle(event.target.value)} />
        </label>
        <fieldset className="grid gap-2">
          <legend className="mb-1 text-xs font-medium text-ink-soft">{t('collab.rooms.selectMembers')}</legend>
          {active.map((agent) => (
            <label key={agent.id} className="flex items-center gap-3 rounded-xl border border-line px-3 py-2 text-sm">
              <input className="size-4 accent-clay" type="checkbox" checked={selected.includes(agent.id)} onChange={() => toggle(agent.id)} />
              <span className="min-w-0 flex-1 truncate">{agent.displayName}</span>
              <span className="text-xs text-ink-soft">@{agent.id}</span>
            </label>
          ))}
          <p className="text-xs text-ink-soft">{t('collab.rooms.minimumMembers')}</p>
        </fieldset>
        {error ? <p className="text-sm text-status-danger-ink">{error}</p> : null}
        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onClose}>{t('common.cancel')}</Button>
          <Button type="submit" variant="accent" disabled={!title.trim() || selected.length < 2}>{t('collab.rooms.createGroup')}</Button>
        </div>
      </form>
    </div>
  )
}
