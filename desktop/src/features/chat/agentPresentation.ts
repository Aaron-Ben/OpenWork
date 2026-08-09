import type { SessionRuntimeView } from './runtimeReducer'
import type { SessionTraceTotals } from './subAgentStore'

export type AgentStatus = 'idle' | 'running' | 'completed' | 'failed' | 'cancelled'

/**
 * 一个智能体当前处于什么状态。
 * 运行时视图里没有"完成度"这种量，所以这里只有离散状态，界面上不要画进度条。
 */
export function agentStatus(
  runtime: SessionRuntimeView | undefined,
  canonicalTurnStatus: string | null = null,
): AgentStatus {
  if (runtime && runtime.phase !== 'idle') return 'running'
  if (runtime?.terminal) return runtime.terminal.status

  // 根会话的实时终态会在正文对账后被回收，卡片需要用已落库的 Turn 状态补回结果。
  switch (canonicalTurnStatus) {
    case 'running':
    case 'completed':
    case 'failed':
    case 'cancelled':
      return canonicalTurnStatus
    default:
      return 'idle'
  }
}

/** 终态摘要。cancelled 没有文本，由调用方补 i18n 文案。 */
export function agentSummary(runtime: SessionRuntimeView | undefined): string | null {
  const terminal = runtime?.terminal
  if (!terminal) return null
  if (terminal.status === 'completed') return terminal.finalText
  if (terminal.status === 'failed') return `${terminal.code}: ${terminal.message}`
  return null
}

/**
 * 智能体累计执行了多久：已落库的轮次时长之和，加上当前在飞那一轮。
 *
 * 两个来源缺一不可。落库汇总每 3 秒才刷一次，且不含在飞的轮次，单用它秒表不会走；
 * 实时视图只有当前这一轮，且是纯内存的，重启或换会话后就没了，单用它历史耗时会退回 --:--。
 *
 * 运行中按当前时刻算，因此调用方要提供 nowMs 而不是让这里读时钟 ——
 * 否则同一次渲染里不同卡片会取到不同的"现在"。
 */
export function agentElapsedMs(
  runtime: SessionRuntimeView | undefined,
  totals: SessionTraceTotals,
  nowMs: number,
): number | null {
  const live = runtime?.startedAtMs == null
    ? null
    : Math.max(0, (runtime.endedAtMs ?? nowMs) - runtime.startedAtMs)

  /*
    两边都覆盖当前 Turn 时以实时值为准，把汇总里同一个 Turn 的部分扣掉。
    少了这一步，轮询追上后会把刚结束的那一轮算两遍；而如果反过来一律信汇总，
    轮次刚结束、轮询还没到的那几秒里秒表会掉回去。
  */
  const overlap = live != null && runtime?.turnId != null && runtime.turnId === totals.latestTurnId
    ? totals.latestTurnMs
    : 0
  const recorded = Math.max(0, totals.runtimeMs - overlap)

  if (live == null) return recorded > 0 ? recorded : null
  return recorded + live
}

export interface AgentToolActivity {
  name: string
  /** 该工具在本 Turn 中的第几次调用，从 1 开始。 */
  attempt: number
}

/** 智能体正在做什么：最后一个工具调用，以及它是这个工具的第几次调用。 */
export function agentToolActivity(
  runtime: SessionRuntimeView | undefined,
): AgentToolActivity | null {
  if (!runtime || runtime.orderedToolCallIds.length === 0) return null
  const lastId = runtime.orderedToolCallIds[runtime.orderedToolCallIds.length - 1]
  const last = runtime.toolCalls[lastId]
  if (!last) return null
  const attempt = runtime.orderedToolCallIds.reduce(
    (count, id) => count + (runtime.toolCalls[id]?.name === last.name ? 1 : 0),
    0,
  )
  return { name: last.name, attempt }
}

/** 顶栏与卡片上的耗时：mm:ss，超过一小时补成 h:mm:ss。 */
export function formatElapsedClock(durationMs: number | null): string {
  if (durationMs == null || !Number.isFinite(durationMs) || durationMs < 0) return '--:--'
  const totalSeconds = Math.floor(durationMs / 1000)
  const seconds = totalSeconds % 60
  const minutes = Math.floor(totalSeconds / 60) % 60
  const hours = Math.floor(totalSeconds / 3600)
  const mmss = `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`
  return hours > 0 ? `${hours}:${mmss}` : mmss
}

/** token 计数：未满一千直接显示，之后用 k / M，保留一位小数。 */
export function formatTokenCount(tokens: number | null | undefined): string {
  if (tokens == null || !Number.isFinite(tokens) || tokens < 0) return '0'
  if (tokens < 1_000) return String(Math.round(tokens))
  if (tokens < 1_000_000) return `${(tokens / 1_000).toFixed(1)}k`
  return `${(tokens / 1_000_000).toFixed(1)}M`
}

/** 智能体徽章上的字：取角色名首个字符，空角色回退成 ?。 */
export function agentInitial(role: string): string {
  const trimmed = role.trim()
  return trimmed ? trimmed.slice(0, 1).toUpperCase() : '?'
}
