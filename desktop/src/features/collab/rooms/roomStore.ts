import { create } from 'zustand'

import { collabCommands, type CollabParticipant, type CollabRoom } from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

type RoomMembersState = Pick<RoomStoreState, 'membersByRoom'>
const EMPTY_ROOM_MEMBERS: CollabParticipant[] = []

export function membersForRoom(state: RoomMembersState, roomId: string): CollabParticipant[] {
  return state.membersByRoom[roomId] ?? EMPTY_ROOM_MEMBERS
}

export function totalUnread(_rooms: CollabRoom[]): number {
  return 0
}

interface RoomStoreState {
  rooms: CollabRoom[]
  membersByRoom: Record<string, CollabParticipant[]>
  loading: boolean
  error: string | null
  fetchAll: () => Promise<void>
  createGroup: (title: string, agentIds: string[]) => Promise<CollabRoom | null>
  openDirect: (agentId: string) => Promise<CollabRoom | null>
  fetchMembers: (roomId: string) => Promise<void>
  addMember: (roomId: string, agentId: string) => Promise<void>
  removeMember: (roomId: string, agentId: string) => Promise<void>
}

export const useRoomStore = create<RoomStoreState>((set, get) => ({
  rooms: [],
  membersByRoom: {},
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
  createGroup: async (title, agentIds) => {
    set({ error: null })
    try {
      const room = await collabCommands.createGroupRoom(title, agentIds)
      await get().fetchAll()
      return room
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
      return null
    }
  },
  openDirect: async (agentId) => {
    set({ error: null })
    try {
      const room = await collabCommands.createDirectRoom(agentId)
      await get().fetchAll()
      return room
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
      return null
    }
  },
  fetchMembers: async (roomId) => {
    set({ error: null })
    try {
      const members = await collabCommands.listRoomMembers(roomId)
      set({ membersByRoom: { ...get().membersByRoom, [roomId]: members } })
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
    }
  },
  addMember: async (roomId, agentId) => {
    set({ error: null })
    try {
      const members = await collabCommands.addGroupMember(roomId, agentId)
      set({ membersByRoom: { ...get().membersByRoom, [roomId]: members } })
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
    }
  },
  removeMember: async (roomId, agentId) => {
    set({ error: null })
    try {
      const members = await collabCommands.removeGroupMember(roomId, agentId)
      set({ membersByRoom: { ...get().membersByRoom, [roomId]: members } })
    } catch (error) {
      set({ error: resolveErrorMessage(error) })
    }
  },
}))
