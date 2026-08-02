import { useCallback, useEffect, useState } from 'react'

import { coreCommands } from '@/bridge/commands'
import { resolveErrorMessage } from '@/lib/commandError'
import { buildCompactionHistory, type CompactionHistoryItem } from './traceViewModel'

export interface CompactionHistoryState {
  items: CompactionHistoryItem[]
  loading: boolean
  error: string | null
}

/**
 * Load a Session's Compaction Spans.
 *
 * A manual compaction has no Turn, so it is unreachable through the Turn-scoped
 * trace queries; this hook is the read path for the Session scope.
 */
export function useCompactionHistory(
  sessionId: string,
  refreshToken = 0,
): CompactionHistoryState {
  const [state, setState] = useState<CompactionHistoryState>({
    items: [],
    loading: true,
    error: null,
  })

  const load = useCallback(async (active: () => boolean) => {
    setState((current) => ({ ...current, loading: true, error: null }))
    try {
      const spans = await coreCommands.listCompactionSpans(sessionId)
      if (!active()) return
      setState({ items: buildCompactionHistory(spans), loading: false, error: null })
    } catch (reason) {
      if (!active()) return
      setState({ items: [], loading: false, error: resolveErrorMessage(reason) })
    }
  }, [sessionId])

  useEffect(() => {
    let active = true
    void load(() => active)
    return () => { active = false }
  }, [load, refreshToken])

  return state
}
