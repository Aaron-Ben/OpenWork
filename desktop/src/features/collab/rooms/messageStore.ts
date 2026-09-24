import { create } from 'zustand'

import { collabCommands, type CollabMessage, type CollabRun } from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'
import { nextViewedSequence } from './roomViewed'

export interface MessageWindow {
  messages: CollabMessage[]
  runs: CollabRun[]
  loading: boolean
  error: string | null
}

interface MessageStoreState {
  byRoom: Record<string, MessageWindow>
  open: (roomId: string) => Promise<void>
  send: (roomId: string, body: string) => Promise<void>
  /** 窗口回到前台时补报这个房间已显示的消息（collaboration-desktop.md §7.2）。 */
  markViewed: (roomId: string) => Promise<void>
}

const openVersions = new Map<string, number>()
/** 每个房间最后一次成功上报的已看到 sequence；上报失败时不更新，下次刷新重试。 */
const reportedViewed = new Map<string, number>()

function windowInForeground(): boolean {
  return document.visibilityState === 'visible' && document.hasFocus()
}

async function reportViewed(roomId: string, messages: CollabMessage[]): Promise<void> {
  const upToSeq = nextViewedSequence(messages, reportedViewed.get(roomId) ?? 0, windowInForeground())
  if (upToSeq === null) return
  const recorded = await collabCommands.markRoomViewed(roomId, upToSeq)
  reportedViewed.set(roomId, recorded)
}

export const useMessageStore = create<MessageStoreState>((set, get) => ({
  byRoom: {},
  open: async (roomId) => {
    const version = (openVersions.get(roomId) ?? 0) + 1
    openVersions.set(roomId, version)
    const current = get().byRoom[roomId] ?? { messages: [], runs: [], loading: false, error: null }
    set({
      byRoom: {
        ...get().byRoom,
        [roomId]: { ...current, loading: current.messages.length === 0, error: null },
      },
    })
    try {
      const [messages, runs] = await Promise.all([
        collabCommands.listMessages(roomId),
        collabCommands.listRuns(),
      ])
      if (openVersions.get(roomId) !== version) return
      set({ byRoom: { ...get().byRoom, [roomId]: { messages, runs, loading: false, error: null } } })
      await reportViewed(roomId, messages)
    } catch (error) {
      if (openVersions.get(roomId) !== version) return
      const latest = get().byRoom[roomId] ?? current
      set({ byRoom: { ...get().byRoom, [roomId]: { ...latest, loading: false, error: resolveErrorMessage(error) } } })
    }
  },
  send: async (roomId, body) => {
    await collabCommands.sendMessage(roomId, body, null)
    await get().open(roomId)
  },
  markViewed: async (roomId) => {
    const shown = get().byRoom[roomId]
    if (!shown) return
    try {
      await reportViewed(roomId, shown.messages)
    } catch {
      // 失败时不记录已上报的位置，下次回到前台或房间刷新时会重试；这里没有需要提示用户的内容。
    }
  },
}))
