import {
  COLLAB_EVENT_VERSION,
  type CollabAgentActivity,
  type CollabEvent,
} from '@/bridge/collab'
import type { HeldNotice } from '@/features/collab/coordinationStore'

export interface CollabEventDependencies {
  refreshAll: () => Promise<void>
  refreshRooms: () => Promise<void>
  refreshAgents: () => Promise<void>
  refreshPermissions: () => Promise<void>
  refreshRoomTail: (roomId: string) => Promise<void>
  applyAgentActivity: (agentId: string, activity: CollabAgentActivity) => void
  recordHeld: (notice: HeldNotice) => void
}

export function createCollabEventController(deps: CollabEventDependencies) {
  let lastSequence = 0

  return {
    process: async (event: CollabEvent): Promise<void> => {
      if (event.version !== COLLAB_EVENT_VERSION || event.sequence !== lastSequence + 1) {
        await deps.refreshAll()
        lastSequence = event.sequence
        return
      }
      lastSequence = event.sequence
      switch (event.type) {
        case 'rooms_changed':
          await Promise.all([deps.refreshRooms(), deps.refreshRoomTail(event.roomId)])
          break
        case 'agents_changed':
          await Promise.all([deps.refreshAgents(), deps.refreshRooms()])
          break
        case 'permissions_changed':
          await deps.refreshPermissions()
          break
        case 'engine_changed':
          await deps.refreshPermissions()
          break
        case 'reply_held':
          deps.recordHeld({
            agentId: event.agentId,
            roomId: event.roomId,
            peerSequence: event.peerSequence,
          })
          break
        case 'agent_activity_changed':
          deps.applyAgentActivity(event.agentId, event.activity)
          break
      }
    },
  }
}
