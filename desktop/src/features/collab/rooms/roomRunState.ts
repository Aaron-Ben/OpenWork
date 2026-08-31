import type { CollabRun } from '@/bridge/collab'

export type RoomRunState = {
  kind: 'thinking' | 'retrying' | 'rate_limited' | 'failed'
  agentIds: string[]
  message: string | null
} | null

function isRateLimited(run: CollabRun): boolean {
  if (run.errorCode === 'ENGINE_RATE_LIMITED' || run.errorCode === 'TRIAGE_RATE_LIMITED') return true
  const detail = `${run.errorCode ?? ''} ${run.errorMessage ?? ''}`.toLowerCase()
  return ['rate limit', 'rate_limit', 'too many requests', '429', 'quota', 'overload']
    .some((marker) => detail.includes(marker))
}

export function roomRunState(runs: CollabRun[], roomId: string): RoomRunState {
  const roomRuns = runs
    .filter((run) => run.roomId === roomId)
    .sort((left, right) => right.startedAt.localeCompare(left.startedAt))
  const active = roomRuns.filter((run) => run.status === 'running')
  if (active.length > 0) {
    const retrying = active.some((run) => {
      const activeIndex = roomRuns.findIndex((candidate) => candidate.id === run.id)
      const prior = roomRuns.slice(activeIndex + 1).find((candidate) => candidate.agentId === run.agentId)
      return prior?.status === 'failed' && isRateLimited(prior)
    })
    return {
      kind: retrying ? 'retrying' : 'thinking',
      agentIds: [...new Set(active.map((run) => run.agentId))],
      message: null,
    }
  }
  const latest = roomRuns[0]
  if (!latest || latest.status !== 'failed') return null
  return {
    kind: isRateLimited(latest) ? 'rate_limited' : 'failed',
    agentIds: [latest.agentId],
    message: latest.errorMessage,
  }
}
