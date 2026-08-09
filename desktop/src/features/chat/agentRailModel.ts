import type { RuntimeSubAgentSessionRecord } from '@/bridge/compat'
import {
  agentElapsedMs,
  agentStatus,
  agentToolActivity,
  type AgentStatus,
  type AgentToolActivity,
} from './agentPresentation'
import type { SessionRuntimeView } from './runtimeReducer'
import { EMPTY_TRACE_TOTALS, type SessionTraceTotals } from './subAgentStore'

export interface AgentRailItem {
  sessionId: string
  /** 卡片主标题：主控是本地化的"主控"，子智能体是它的 agentRole。 */
  role: string
  /** 卡片副标题：主控是会话标题，子智能体是 taskName。 */
  task: string
  status: AgentStatus
  toolActivity: AgentToolActivity | null
  elapsedMs: number | null
  tokens: number
  /** 模型调用 + 工具调用次数。 */
  steps: number
  /** 主控卡片。列表里恒定排在第一位。 */
  orchestrator: boolean
}

export interface AgentRailInput {
  parentSessionId: string
  /** 主控卡片的主标题，由调用方做 i18n。 */
  orchestratorRole: string
  /** 主控卡片的副标题，通常是会话标题。 */
  orchestratorTask: string
  children: readonly RuntimeSubAgentSessionRecord[]
  runtimeBySession: Record<string, SessionRuntimeView>
  totalsBySession: Record<string, SessionTraceTotals>
  nowMs: number
}

/**
 * 右栏要展示的卡片列表：主控在前，子智能体按 listSubAgents 的顺序跟随。
 * 纯函数 —— 时间由调用方传入，同一次渲染里所有卡片读到同一个"现在"。
 */
export function buildAgentRailItems(input: AgentRailInput): AgentRailItem[] {
  const { runtimeBySession, totalsBySession, nowMs } = input
  const totalsFor = (sessionId: string) => totalsBySession[sessionId] ?? EMPTY_TRACE_TOTALS
  const orchestratorTotals = totalsFor(input.parentSessionId)
  const orchestrator: AgentRailItem = {
    sessionId: input.parentSessionId,
    role: input.orchestratorRole,
    task: input.orchestratorTask,
    status: agentStatus(
      runtimeBySession[input.parentSessionId],
      orchestratorTotals.latestTurnStatus,
    ),
    toolActivity: agentToolActivity(runtimeBySession[input.parentSessionId]),
    elapsedMs: agentElapsedMs(runtimeBySession[input.parentSessionId], nowMs),
    tokens: orchestratorTotals.tokens,
    steps: orchestratorTotals.steps,
    orchestrator: true,
  }
  const children = input.children.map((child): AgentRailItem => ({
    sessionId: child.id,
    role: child.agentRole,
    task: child.taskName,
    status: agentStatus(runtimeBySession[child.id]),
    toolActivity: agentToolActivity(runtimeBySession[child.id]),
    elapsedMs: agentElapsedMs(runtimeBySession[child.id], nowMs),
    tokens: totalsFor(child.id).tokens,
    steps: totalsFor(child.id).steps,
    orchestrator: false,
  }))
  return [orchestrator, ...children]
}

export interface AgentRailCounts {
  running: number
  /** 当前没有执行 Turn，且最近一次没有失败或取消的智能体。 */
  standby: number
}

export function agentRailCounts(items: readonly AgentRailItem[]): AgentRailCounts {
  return items.reduce<AgentRailCounts>(
    (counts, item) => ({
      running: counts.running + (item.status === 'running' ? 1 : 0),
      standby: counts.standby + (
        item.status === 'idle' || item.status === 'completed' ? 1 : 0
      ),
    }),
    { running: 0, standby: 0 },
  )
}

export function agentRailTotalTokens(items: readonly AgentRailItem[]): number {
  return items.reduce((total, item) => total + item.tokens, 0)
}

/** 整棵树走了多少步：主控与所有子智能体的模型调用 + 工具调用之和。 */
export function agentRailTotalSteps(items: readonly AgentRailItem[]): number {
  return items.reduce((total, item) => total + item.steps, 0)
}

/** 整棵树跑了多久：以最早开始的智能体为准。 */
export function agentRailElapsedMs(items: readonly AgentRailItem[]): number | null {
  const durations = items.flatMap((item) => item.elapsedMs == null ? [] : [item.elapsedMs])
  return durations.length === 0 ? null : Math.max(...durations)
}

export function findAgentRailItem(
  items: readonly AgentRailItem[],
  sessionId: string | null,
): AgentRailItem | null {
  if (!sessionId) return null
  return items.find((item) => item.sessionId === sessionId) ?? null
}
