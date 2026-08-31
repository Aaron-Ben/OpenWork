import { create } from 'zustand'

import {
  collabCommands,
  type CollabRun,
  type CollabRunTrace,
} from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

interface ObservabilityState {
  runs: CollabRun[]
  trace: CollabRunTrace | null
  selectedRunId: string | null
  agentFilter: string | null
  statusFilter: string | null
  loading: boolean
  error: string | null
  setAgentFilter: (agentId: string | null) => void
  setStatusFilter: (status: string | null) => void
  selectRun: (runId: string) => Promise<void>
  refresh: () => Promise<void>
}

let requestVersion = 0

export const useObservabilityStore = create<ObservabilityState>((set, get) => ({
  runs: [],
  trace: null,
  selectedRunId: null,
  agentFilter: null,
  statusFilter: null,
  loading: false,
  error: null,
  setAgentFilter: (agentFilter) => set({ agentFilter }),
  setStatusFilter: (statusFilter) => set({ statusFilter }),
  selectRun: async (selectedRunId) => {
    const version = ++requestVersion
    set({ selectedRunId, error: null })
    try {
      const trace = await collabCommands.getRunTrace(selectedRunId)
      if (version === requestVersion && get().selectedRunId === selectedRunId) set({ trace })
    } catch (error) {
      if (version === requestVersion && get().selectedRunId === selectedRunId) {
        set({ error: resolveErrorMessage(error) })
      }
    }
  },
  refresh: async () => {
    const version = ++requestVersion
    const { agentFilter, statusFilter, selectedRunId } = get()
    set({ loading: get().runs.length === 0, error: null })
    try {
      const runs = await collabCommands.listRuns({
        agentId: agentFilter,
        status: statusFilter,
      })
      const nextSelectedRunId = runs.some((run) => run.id === selectedRunId)
        ? selectedRunId
        : runs[0]?.id ?? null
      const trace = nextSelectedRunId
        ? await collabCommands.getRunTrace(nextSelectedRunId)
        : null
      const current = get()
      if (version === requestVersion
        && current.agentFilter === agentFilter
        && current.statusFilter === statusFilter) {
        set({
          runs,
          trace,
          selectedRunId: nextSelectedRunId,
          loading: false,
        })
      }
    } catch (error) {
      if (version === requestVersion) {
        set({ error: resolveErrorMessage(error), loading: false })
      }
    }
  },
}))
