import { useEffect } from 'react'

import {
  listenToCollabInvalidations,
  type CollabInvalidation,
} from '@/bridge/collabEvents'
import { useAgentStore } from '@/features/collab/agents/agentStore'
import { useBoardStore } from '@/features/collab/boards/boardStore'
import { useCollabNavigationStore, type CollabView } from '@/features/collab/collabNavigationStore'
import { useMessageStore } from '@/features/collab/rooms/messageStore'
import { useRoomStore } from '@/features/collab/rooms/roomStore'
import { useCollabRuntimeStore } from '@/features/collab/runtimeStore'

export interface InvalidationContext {
  view: CollabView
  activeRoomId: string | null
  activeRoomKind: 'direct' | 'group' | null
}

export interface InvalidationRefreshers {
  runtime: () => Promise<unknown>
  agents: () => Promise<unknown>
  rooms: () => Promise<unknown>
  roomMembers: (roomId: string) => Promise<unknown>
  messages: (roomId: string) => Promise<unknown>
  boards: () => Promise<unknown>
}

export async function refreshForInvalidation(
  invalidation: CollabInvalidation,
  context: InvalidationContext,
  refreshers: InvalidationRefreshers,
): Promise<void> {
  const tasks: Array<Promise<unknown>> = []
  const refreshActiveMessages = () => {
    if (context.activeRoomId) tasks.push(refreshers.messages(context.activeRoomId))
  }

  switch (invalidation.kind) {
    case 'runtime_ready':
      tasks.push(refreshers.runtime(), refreshers.agents(), refreshers.rooms())
      refreshActiveMessages()
      if (context.view === 'boards') tasks.push(refreshers.boards())
      break
    case 'agent_config':
      tasks.push(refreshers.runtime(), refreshers.agents())
      break
    case 'room':
      tasks.push(refreshers.rooms())
      if (context.activeRoomId
        && context.activeRoomKind === 'group'
        && (!invalidation.subjectId || invalidation.subjectId === context.activeRoomId)) {
        tasks.push(refreshers.roomMembers(context.activeRoomId))
      }
      break
    case 'message':
      // The room list is ordered by last activity, so every committed message
      // can move a row even when its room is not currently open.
      tasks.push(refreshers.rooms())
      if (context.activeRoomId
        && (!invalidation.subjectId || invalidation.subjectId === context.activeRoomId)) {
        tasks.push(refreshers.messages(context.activeRoomId))
      }
      break
    case 'board':
      if (context.view === 'boards') tasks.push(refreshers.boards())
      break
    case 'engine_inventory':
      tasks.push(refreshers.runtime())
      break
    case 'runner_status':
      tasks.push(refreshers.runtime())
      refreshActiveMessages()
      break
  }

  await Promise.all(tasks)
}

/**
 * Owns the single WebView-side invalidation subscription. Feature modules
 * expose refresh operations; transport routing stays out of view components.
 */
export function useCollabInvalidationCoordinator(): void {
  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined
    void listenToCollabInvalidations((invalidation) => {
      const navigation = useCollabNavigationStore.getState()
      const rooms = useRoomStore.getState()
      const activeRoom = rooms.rooms.find((room) => room.id === navigation.activeRoomId)
      void refreshForInvalidation(
        invalidation,
        {
          view: navigation.view,
          activeRoomId: navigation.activeRoomId,
          activeRoomKind: activeRoom?.kind ?? null,
        },
        {
          runtime: useCollabRuntimeStore.getState().fetch,
          agents: useAgentStore.getState().fetchAll,
          rooms: rooms.fetchAll,
          roomMembers: rooms.fetchMembers,
          messages: useMessageStore.getState().open,
          boards: useBoardStore.getState().fetchAll,
        },
      )
    }).then((stop) => {
      if (disposed) stop()
      else unlisten = stop
    })
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])
}
