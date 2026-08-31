import type { CollabAgent, CollabRuntimeStatus } from '@/bridge/collab'

export type AgentActualState =
  | { kind: 'running' }
  | { kind: 'starting' | 'restarting' | 'engineMissing' | 'engineError' }
  | { kind: 'error'; detail: string | null }

export function agentActualState(runtime: CollabRuntimeStatus | null, agent: CollabAgent): AgentActualState {
  const runner = runtime?.runners.find((candidate) => candidate.agentId === agent.id)
  if (runner?.state === 'error') return { kind: 'error', detail: runner.lastError }
  if (runner?.state === 'running') {
    return runner.configRevision === agent.configRevision
      ? { kind: 'running' }
      : { kind: 'restarting' }
  }
  const readiness = runtime?.engineReadiness.find((engine) => engine.engineId === agent.engineId)
  if (readiness?.status === 'missing') return { kind: 'engineMissing' }
  if (readiness?.status === 'error') return { kind: 'engineError' }
  return { kind: 'starting' }
}
