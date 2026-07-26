import { create } from 'zustand'

import { coreCommands } from '../bridge/commands'
import type { RuntimeTraceContentPolicy } from '../bridge/compat'
import { resolveErrorMessage } from '../utils/commandError'

export const TRACE_CONTENT_POLICY_STORAGE_KEY = 'openwork-trace-content-policy'
export const DEFAULT_TRACE_CONTENT_POLICY: RuntimeTraceContentPolicy = 'full'

type TraceContentSyncState = 'idle' | 'syncing' | 'ready' | 'error'

interface TraceContentStoreState {
  policy: RuntimeTraceContentPolicy
  syncState: TraceContentSyncState
  updating: boolean
  error: string | null
  initialize: () => Promise<boolean>
  updatePolicy: (policy: RuntimeTraceContentPolicy) => Promise<boolean>
}

export function parseTraceContentPolicy(value: unknown): RuntimeTraceContentPolicy | null {
  return value === 'full' || value === 'compaction_only' || value === 'off' ? value : null
}

function readStoredTraceContentPolicy(): RuntimeTraceContentPolicy {
  if (typeof localStorage === 'undefined') return DEFAULT_TRACE_CONTENT_POLICY
  return parseTraceContentPolicy(localStorage.getItem(TRACE_CONTENT_POLICY_STORAGE_KEY))
    ?? DEFAULT_TRACE_CONTENT_POLICY
}

export const useTraceContentStore = create<TraceContentStoreState>((set, get) => ({
  policy: readStoredTraceContentPolicy(),
  syncState: 'idle',
  updating: false,
  error: null,

  initialize: async () => {
    if (get().syncState === 'ready') return true
    if (get().syncState === 'syncing') return false
    set({ syncState: 'syncing', error: null })
    try {
      const policy = await coreCommands.setTraceContentPolicy(get().policy)
      set({ policy, syncState: 'ready', error: null })
      return true
    } catch (error) {
      set({ syncState: 'error', error: resolveErrorMessage(error) })
      return false
    }
  },

  updatePolicy: async (policy) => {
    if (get().updating || policy === get().policy) return policy === get().policy
    const previousPolicy = get().policy
    set({ updating: true, error: null })
    try {
      if (typeof localStorage !== 'undefined') {
        localStorage.setItem(TRACE_CONTENT_POLICY_STORAGE_KEY, policy)
      }
    } catch (error) {
      set({ updating: false, error: resolveErrorMessage(error) })
      return false
    }
    try {
      const applied = await coreCommands.setTraceContentPolicy(policy)
      if (typeof localStorage !== 'undefined' && applied !== policy) {
        localStorage.setItem(TRACE_CONTENT_POLICY_STORAGE_KEY, applied)
      }
      set({ policy: applied, syncState: 'ready', updating: false, error: null })
      return true
    } catch (error) {
      try {
        if (typeof localStorage !== 'undefined') {
          localStorage.setItem(TRACE_CONTENT_POLICY_STORAGE_KEY, previousPolicy)
        }
      } catch {
        // Keep the runtime error visible. Startup still blocks turns until its
        // persisted policy can be pushed successfully.
      }
      set({ updating: false, error: resolveErrorMessage(error) })
      return false
    }
  },
}))
