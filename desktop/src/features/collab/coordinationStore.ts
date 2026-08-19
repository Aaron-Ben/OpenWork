import { create } from 'zustand'

export interface HeldNotice {
  agentId: string
  roomId: string
  peerSequence: number
}

interface CoordinationStoreState {
  heldByRoom: Record<string, HeldNotice>
  recordHeld: (notice: HeldNotice) => void
}

export const useCoordinationStore = create<CoordinationStoreState>((set, get) => ({
  heldByRoom: {},
  recordHeld: (notice) => {
    set({
      heldByRoom: {
        ...get().heldByRoom,
        [notice.roomId]: notice,
      },
    })
  },
}))
