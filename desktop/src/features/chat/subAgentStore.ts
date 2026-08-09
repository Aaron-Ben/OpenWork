import { create } from 'zustand'

import { coreCommands } from '@/bridge/commands'
import type { RuntimeSubAgentSessionRecord } from '@/bridge/compat'
import { resolveErrorMessage } from '@/lib/commandError'
import { useRuntimeStore } from './runtimeStore'

/** 列表刷新间隔：父 Turn 运行中时子 Agent 的状态变化最需要跟上。 */
export const SUB_AGENT_LIST_INTERVAL_MS = 1_000
/** Trace 汇总要为每个会话拉一次列表，比状态贵得多，刷得更慢。 */
export const SUB_AGENT_TOKEN_INTERVAL_MS = 3_000
const TOKEN_TRACE_LIMIT = 50

/** 一个会话已记录的 Trace 汇总。 */
export interface SessionTraceTotals {
  /** 各 Trace 的 token 合计。跨 provider 不可直接比较，只用于同一棵树内的展示。 */
  tokens: number
  /** 模型调用 + 工具调用的次数，也就是顶栏说的"N 步"。 */
  steps: number
  /** 最新一条有归属 Turn 的 Trace 所对应的规范 Turn 状态。 */
  latestTurnStatus: string | null
}

export const EMPTY_TRACE_TOTALS: SessionTraceTotals = {
  tokens: 0,
  steps: 0,
  latestTurnStatus: null,
}

export interface SubAgentEntry {
  children: RuntimeSubAgentSessionRecord[]
  /** 列表读取失败。失败时保留上一次的 children，避免界面看起来像子 Agent 全没了。 */
  error: string | null
  /** 会话 id → Trace 汇总。含父会话自己。 */
  totalsBySession: Record<string, SessionTraceTotals>
}

export const EMPTY_SUB_AGENT_ENTRY: SubAgentEntry = {
  children: [],
  error: null,
  totalsBySession: {},
}

interface SubAgentStoreState {
  byParent: Record<string, SubAgentEntry>
  refresh: (parentSessionId: string) => Promise<void>
  refreshTotals: (parentSessionId: string) => Promise<void>
  clear: (parentSessionId: string) => void
}

/*
  后发先至的响应不能覆盖更新的响应：每次请求领一个序号，回来时序号已经被后续请求
  顶掉就直接丢弃。父 Turn 运行中每秒发一次，这种交错是常态而不是边界情况。
*/
const listSequence = new Map<string, number>()
const tokenSequence = new Map<string, number>()

function nextSequence(counters: Map<string, number>, key: string): number {
  const next = (counters.get(key) ?? 0) + 1
  counters.set(key, next)
  return next
}

function entryFor(state: SubAgentStoreState, parentSessionId: string): SubAgentEntry {
  return state.byParent[parentSessionId] ?? EMPTY_SUB_AGENT_ENTRY
}

/** 一个会话已记录的 Trace 汇总。没有 Trace 的会话是全零，不是缺失。 */
export async function sessionTraceTotals(sessionId: string): Promise<SessionTraceTotals> {
  const summaries = await coreCommands.listTraces(sessionId, TOKEN_TRACE_LIMIT)
  let tokens = 0
  let steps = 0
  let latestTurnSequence: number | null = null
  let latestTurnStatus: string | null = null

  for (const summary of summaries) {
    tokens += summary.totalTokens ?? 0
    steps += summary.modelCallCount + summary.toolCallCount
    // 手动压缩与 rewind 没有 Turn，不能让它们覆盖真正的会话终态。
    if (summary.turnId === null || summary.turnSequence === null) continue
    if (latestTurnSequence !== null && summary.turnSequence <= latestTurnSequence) continue
    latestTurnSequence = summary.turnSequence
    latestTurnStatus = summary.status
  }

  return { tokens, steps, latestTurnStatus }
}

export const useSubAgentStore = create<SubAgentStoreState>((set, get) => ({
  byParent: {},

  refresh: async (parentSessionId) => {
    const sequence = nextSequence(listSequence, parentSessionId)
    try {
      const listed = await coreCommands.listSubAgents(parentSessionId)
      if (listSequence.get(parentSessionId) !== sequence) return
      set((state) => ({
        byParent: {
          ...state.byParent,
          [parentSessionId]: {
            ...entryFor(state, parentSessionId),
            children: listed,
            error: null,
          },
        },
      }))
      // 子 Agent 的运行时事件靠这张父子表分流，不注册的话卡片状态会永远停在 idle。
      useRuntimeStore.getState().registerSubAgents(
        parentSessionId,
        listed.map((child) => child.id),
      )
    } catch (error) {
      if (listSequence.get(parentSessionId) !== sequence) return
      const message = resolveErrorMessage(error)
      set((state) => ({
        byParent: {
          ...state.byParent,
          [parentSessionId]: { ...entryFor(state, parentSessionId), error: message },
        },
      }))
    }
  },

  refreshTotals: async (parentSessionId) => {
    const sequence = nextSequence(tokenSequence, parentSessionId)
    const sessionIds = [
      parentSessionId,
      ...entryFor(get(), parentSessionId).children.map((child) => child.id),
    ]
    try {
      const totals = await Promise.all(
        sessionIds.map(async (sessionId) => [sessionId, await sessionTraceTotals(sessionId)] as const),
      )
      if (tokenSequence.get(parentSessionId) !== sequence) return
      set((state) => ({
        byParent: {
          ...state.byParent,
          [parentSessionId]: {
            ...entryFor(state, parentSessionId),
            totalsBySession: Object.fromEntries(totals),
          },
        },
      }))
    } catch {
      // Trace 汇总是观测性的，读不到就沿用上一次的数值，绝不能影响会话本身。
    }
  },

  clear: (parentSessionId) => {
    listSequence.delete(parentSessionId)
    tokenSequence.delete(parentSessionId)
    set((state) => {
      if (!(parentSessionId in state.byParent)) return state
      const byParent = { ...state.byParent }
      delete byParent[parentSessionId]
      return { byParent }
    })
  },
}))

interface PollingRecord {
  refCount: number
  live: boolean
  listTimer: ReturnType<typeof setInterval> | null
  tokenTimer: ReturnType<typeof setInterval> | null
}

/*
  右栏和子智能体详情页读的是同一份数据。定时器按父会话引用计数，多个消费者也只跑一份 ——
  否则每多挂一个组件就多一条每秒轮询。
*/
const polling = new Map<string, PollingRecord>()

function stopTimers(record: PollingRecord) {
  if (record.listTimer !== null) clearInterval(record.listTimer)
  if (record.tokenTimer !== null) clearInterval(record.tokenTimer)
  record.listTimer = null
  record.tokenTimer = null
}

function startTimers(parentSessionId: string, record: PollingRecord) {
  stopTimers(record)
  record.listTimer = setInterval(() => {
    void useSubAgentStore.getState().refresh(parentSessionId)
  }, SUB_AGENT_LIST_INTERVAL_MS)
  record.tokenTimer = setInterval(() => {
    void useSubAgentStore.getState().refreshTotals(parentSessionId)
  }, SUB_AGENT_TOKEN_INTERVAL_MS)
}

function refreshNow(parentSessionId: string) {
  const store = useSubAgentStore.getState()
  void store.refresh(parentSessionId).then(() => store.refreshTotals(parentSessionId))
}

/** 订阅一个父会话的子 Agent 数据。返回退订函数；引用计数归零时停表，数据保留。 */
export function retainSubAgents(parentSessionId: string): () => void {
  const existing = polling.get(parentSessionId)
  if (existing) {
    existing.refCount += 1
  } else {
    polling.set(parentSessionId, {
      refCount: 1,
      live: false,
      listTimer: null,
      tokenTimer: null,
    })
  }
  refreshNow(parentSessionId)

  let released = false
  return () => {
    if (released) return
    released = true
    const record = polling.get(parentSessionId)
    if (!record) return
    record.refCount -= 1
    if (record.refCount > 0) return
    stopTimers(record)
    polling.delete(parentSessionId)
  }
}

/** 父 Turn 是否在跑。只有在跑的时候才轮询；状态翻转时补一次即时刷新。 */
export function setSubAgentPolling(parentSessionId: string, live: boolean): void {
  const record = polling.get(parentSessionId)
  if (!record || record.live === live) return
  record.live = live
  if (live) startTimers(parentSessionId, record)
  else stopTimers(record)
  refreshNow(parentSessionId)
}

/** 仅供测试：清掉定时器与引用计数，避免用例之间互相污染。 */
export function resetSubAgentPolling(): void {
  for (const record of polling.values()) stopTimers(record)
  polling.clear()
  listSequence.clear()
  tokenSequence.clear()
  useSubAgentStore.setState({ byParent: {} })
}

export function subAgentPollingRefCount(parentSessionId: string): number {
  return polling.get(parentSessionId)?.refCount ?? 0
}

export function subAgentPollingActive(parentSessionId: string): boolean {
  return polling.get(parentSessionId)?.listTimer != null
}
