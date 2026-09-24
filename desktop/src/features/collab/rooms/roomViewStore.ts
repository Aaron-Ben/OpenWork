import { create } from 'zustand'

import type { CollabRoomMessage } from '@/bridge/collab'

/** 房间右侧栏显示什么（collaboration-desktop.md §7.5）。 */
export type RoomPanel =
  | { kind: 'info' }
  | { kind: 'card', cardId: string }
  | { kind: 'agent', agentId: string }

/** 房间页的纯界面状态：右侧栏、正在引用的消息、跳转后高亮的消息。换房间时清空。 */
interface RoomViewState {
  panel: RoomPanel
  panelOpen: boolean
  quoting: CollabRoomMessage | null
  highlightedMessageId: string | null
  /** 成员管理对话框是否打开（群组）。 */
  managing: boolean
  /** 每次发送后加一；消息流据此滚到最新。 */
  followToken: number
  showCard: (cardId: string) => void
  showAgent: (agentId: string) => void
  closePanel: () => void
  togglePanelOpen: () => void
  quote: (message: CollabRoomMessage | null) => void
  highlight: (messageId: string | null) => void
  setManaging: (managing: boolean) => void
  followLatest: () => void
  reset: () => void
}

const INFO: RoomPanel = { kind: 'info' }

export const useRoomViewStore = create<RoomViewState>((set) => ({
  panel: INFO,
  panelOpen: true,
  quoting: null,
  highlightedMessageId: null,
  managing: false,
  followToken: 0,
  showCard: (cardId) => set({ panel: { kind: 'card', cardId }, panelOpen: true }),
  showAgent: (agentId) => set({ panel: { kind: 'agent', agentId }, panelOpen: true }),
  closePanel: () => set({ panel: INFO }),
  togglePanelOpen: () => set((state) => ({ panelOpen: !state.panelOpen })),
  quote: (quoting) => set({ quoting }),
  highlight: (highlightedMessageId) => set({ highlightedMessageId }),
  setManaging: (managing) => set({ managing }),
  followLatest: () => set((state) => ({ followToken: state.followToken + 1 })),
  reset: () => set({ panel: INFO, quoting: null, highlightedMessageId: null, managing: false }),
}))
