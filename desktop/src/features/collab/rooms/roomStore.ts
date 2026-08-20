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
  create: (title: string) => Promise<void>
  addMember: (roomId: string, participantId: string) => Promise<void>
  removeMember: (roomId: string, participantId: string) => Promise<void>
  setMuted: (roomId: string, participantId: string, muted: boolean) => Promise<void>
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
  create: async (title) => {
    await collabCommands.createRoom(title)
    await get().fetchAll()
  },
  addMember: async (roomId, participantId) => {
    await collabCommands.addMember(roomId, participantId)
    await get().fetchAll()
  },
  removeMember: async (roomId, participantId) => {
    await collabCommands.removeMember(roomId, participantId)
    await get().fetchAll()
  },
  setMuted: async (roomId, participantId, muted) => {
    await collabCommands.setMuted(roomId, participantId, muted)
    await get().fetchAll()
  },
  markRead: async (roomId, throughSequence) => {
    await collabCommands.markRead(roomId, throughSequence)
    await get().fetchAll()
  },
}))
