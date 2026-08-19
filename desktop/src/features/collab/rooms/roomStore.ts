import { create } from 'zustand'

import { collabCommands, type CollabRoomSummary } from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

export function totalUnread(rooms: CollabRoomSummary[]): number {
  return rooms.reduce((total, room) => total + (room.muted ? 0 : room.unreadCount), 0)
}

interface RoomStoreState {
  rooms: CollabRoomSummary[]
  loading: boolean
  error: string | null
  fetchAll: () => Promise<void>
  create: (id: string, title: string) => Promise<void>
  addMember: (roomId: string, participantId: string) => Promise<void>
  markRead: (roomId: string, throughSequence: number) => Promise<void>
}

export const useRoomStore = create<RoomStoreState>((set, get) => ({
  rooms: [],
  loading: false,
  error: null,
  fetchAll: async () => {
    set({ loading: true, error: null })
    try {
      set({ rooms: await collabCommands.listRooms(), loading: false })
    } catch (error) {
      set({ error: resolveErrorMessage(error), loading: false })
    }
  },
  create: async (id, title) => {
    await collabCommands.createRoom(id, title)
    await get().fetchAll()
  },
  addMember: async (roomId, participantId) => {
    await collabCommands.addMember(roomId, participantId)
    await get().fetchAll()
  },
  markRead: async (roomId, throughSequence) => {
    await collabCommands.markRead(roomId, throughSequence)
    await get().fetchAll()
  },
}))
