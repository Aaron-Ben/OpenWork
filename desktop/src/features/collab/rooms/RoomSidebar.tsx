import type { CollabAgent, CollabRoomSummary } from '@/bridge/collab'
import { AgentProfilePanel } from './AgentProfilePanel'
import { CardPreviewPanel } from './CardPreviewPanel'
import { RoomInfoPanel } from './RoomInfoPanel'
import { useRoomViewStore } from './roomViewStore'

/** 房间右侧栏（collaboration-desktop.md §7.5）：房间信息、卡片预览或 Agent 资料。 */
export function RoomSidebar({ room, members, agents }: {
  room: CollabRoomSummary
  members: CollabAgent[]
  agents: CollabAgent[]
}) {
  const panel = useRoomViewStore((state) => state.panel)
  return (
    <aside className="flex w-[300px] shrink-0 flex-col border-l border-line bg-paper-hover">
      {panel.kind === 'card' ? <CardPreviewPanel cardId={panel.cardId} agents={agents} />
        : panel.kind === 'agent' ? <AgentProfilePanel agentId={panel.agentId} agents={agents} />
          : <RoomInfoPanel room={room} members={members} agents={agents} />}
    </aside>
  )
}
