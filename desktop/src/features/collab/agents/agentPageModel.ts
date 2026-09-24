import type { CollabAgent, CollabRuntimeStatus } from '@/bridge/collab'
import type { StatusTone } from '@/features/collab/components/agentStatus'

export type AgentTab = 'active' | 'archived'

/** 活跃或已归档的 Agent，保持 Server 的顺序。 */
export function agentsInTab(agents: CollabAgent[], tab: AgentTab): CollabAgent[] {
  return agents.filter((agent) => (agent.archivedAt === null) === (tab === 'active'))
}

export interface StatusCounts {
  working: number
  queued: number
  idle: number
  error: number
}

/** 标题栏的计数（collaboration-desktop.md §8）：活跃 Agent 按当前状态分组。 */
export function statusCounts(agents: CollabAgent[]): StatusCounts {
  const counts: StatusCounts = { working: 0, queued: 0, idle: 0, error: 0 }
  for (const agent of agents) {
    const kind = agent.activity.kind
    if (kind !== 'archived') counts[kind] += 1
  }
  return counts
}

export interface EngineTag {
  tone: StatusTone
  key: `collab.agents.engine.${'ready' | 'missing' | 'error' | 'checking'}`
  values: Record<string, string>
}

/**
 * 标题栏的 Engine 标签。OpenCode 就绪即表示沙箱自检已通过（collaboration.md §3.1：沙箱不可用时
 * inventory 为 error），所以就绪时写“沙箱已启用”。
 */
export function engineTag(runtime: CollabRuntimeStatus | null): EngineTag {
  const engine = runtime?.engines.find((candidate) => candidate.engineId === 'opencode')
  if (!engine) return { tone: 'neutral', key: 'collab.agents.engine.checking', values: {} }
  switch (engine.status) {
    case 'ready': return { tone: 'success', key: 'collab.agents.engine.ready', values: { version: engine.version ?? '' } }
    case 'missing': return { tone: 'danger', key: 'collab.agents.engine.missing', values: {} }
    case 'error': return { tone: 'danger', key: 'collab.agents.engine.error', values: { reason: engine.lastError ?? '' } }
    case 'unknown':
      return { tone: 'neutral', key: 'collab.agents.engine.checking', values: {} }
    default: {
      const unreachable: never = engine.status
      return unreachable
    }
  }
}
