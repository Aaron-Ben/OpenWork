import { useEffect } from 'react'

import { useRuntimeStore } from './runtimeStore'
import {
  EMPTY_SUB_AGENT_ENTRY,
  retainSubAgents,
  setSubAgentPolling,
  useSubAgentStore,
  type SubAgentEntry,
} from './subAgentStore'

/**
 * 订阅一个父会话的子 Agent 列表与 token 合计。
 * 多个组件传同一个 parentSessionId 时共用一份轮询（见 subAgentStore 的引用计数）。
 */
export function useSubAgents(parentSessionId: string | null): SubAgentEntry {
  const entry = useSubAgentStore((state) => (
    parentSessionId
      ? state.byParent[parentSessionId] ?? EMPTY_SUB_AGENT_ENTRY
      : EMPTY_SUB_AGENT_ENTRY
  ))
  const live = useRuntimeStore((state) => (
    parentSessionId ? (state.bySession[parentSessionId]?.phase ?? 'idle') !== 'idle' : false
  ))

  useEffect(() => {
    if (!parentSessionId) return
    return retainSubAgents(parentSessionId)
  }, [parentSessionId])

  useEffect(() => {
    if (!parentSessionId) return
    setSubAgentPolling(parentSessionId, live)
  }, [live, parentSessionId])

  return entry
}
