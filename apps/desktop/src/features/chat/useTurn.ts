import { useCallback } from 'react'

import { coreCommands } from '../../bridge/commands'
import type { RuntimePermissionDecision, RuntimePermissionMode } from '../../bridge/compat'
import { useContextWindowStore } from '../../stores/contextWindowStore'
import { resolveErrorMessage } from '../../utils/commandError'
import { useRuntimeStore } from './runtimeStore'

function nextClientRequestId(): string {
  return `request-${crypto.randomUUID().split('-').join('')}`
}

export function useTurnActions(sessionId: string | null) {
  const startTurn = useCallback(async (text: string) => {
    if (!sessionId) return false
    const clientRequestId = nextClientRequestId()
    if (!useRuntimeStore.getState().beginTurn(sessionId, clientRequestId, text)) return false
    try {
      const accepted = await coreCommands.startTurn(
        sessionId,
        clientRequestId,
        text.trim(),
        useContextWindowStore.getState().contextWindowTokens,
      )
      useRuntimeStore.getState().acceptTurn(sessionId, clientRequestId, accepted.turnId)
      return true
    } catch (error) {
      useRuntimeStore.getState().failTurnStart(
        sessionId,
        clientRequestId,
        resolveErrorMessage(error),
      )
      return false
    }
  }, [sessionId])

  const cancelTurn = useCallback(async () => {
    if (!sessionId) return
    const runtime = useRuntimeStore.getState().bySession[sessionId]
    if (!runtime?.turnId) return
    try {
      await coreCommands.cancelTurn(sessionId, runtime.turnId)
    } catch (error) {
      useRuntimeStore.getState().setError(sessionId, resolveErrorMessage(error))
    }
  }, [sessionId])

  const resolvePermission = useCallback(async (decision: RuntimePermissionDecision) => {
    if (!sessionId) return false
    const runtime = useRuntimeStore.getState().bySession[sessionId]
    const request = runtime?.pendingPermission
    if (!runtime?.turnId || !request) return false
    try {
      await coreCommands.resolvePermission(
        sessionId,
        runtime.turnId,
        request.toolCallId,
        decision,
      )
      return true
    } catch (error) {
      useRuntimeStore.getState().setError(sessionId, resolveErrorMessage(error))
      return false
    }
  }, [sessionId])

  const setPermissionMode = useCallback(async (mode: RuntimePermissionMode) => {
    if (!sessionId) return false
    try {
      const applied = await coreCommands.setPermissionMode(sessionId, mode)
      useRuntimeStore.getState().setPermissionMode(sessionId, applied)
      return true
    } catch (error) {
      useRuntimeStore.getState().setError(sessionId, resolveErrorMessage(error))
      return false
    }
  }, [sessionId])

  return { startTurn, cancelTurn, resolvePermission, setPermissionMode }
}
