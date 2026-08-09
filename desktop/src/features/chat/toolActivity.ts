import type { ToolResultArtifact, ToolResultState } from '@/types/parts'

export type ToolActivityState = ToolResultState | 'pending' | 'submitted' | 'finished'

export interface ToolActivity {
  id: string
  name: string
  input: Record<string, unknown> | null
  rawInput: string
  output: string
  state: ToolActivityState
  artifacts: ToolResultArtifact[]
  sequence: number
  separatedBefore: boolean
}

export function isFailure(activity: ToolActivity): boolean {
  return activity.state === 'error'
    || activity.state === 'denied'
    || activity.state === 'interrupted'
}

export function isInProgress(activity: ToolActivity): boolean {
  return activity.state === 'pending'
    || activity.state === 'submitted'
    || activity.state === 'running'
}
