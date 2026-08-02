import { useEffect } from 'react'

import { listenToSessionUpdates } from '@/bridge/events'
import {
  supportsRuntimeSessionUpdateVersion,
  type RuntimeSessionUpdateEnvelope,
} from '@/bridge/compat'
import { useRuntimeStore } from '@/features/chat/runtimeStore'
import { useSessionStore } from '@/features/sessions/sessionStore'
import { resolveErrorMessage } from '@/lib/commandError'
import { processSessionUpdate } from './coreEventController'

const LIVE_UPDATE_BATCH_MS = 100

interface SessionUpdateDispatcherDependencies {
  applyLiveBatch: (
    payloads: RuntimeSessionUpdateEnvelope[],
  ) => void | Promise<void>
  processUpdate: (payload: RuntimeSessionUpdateEnvelope) => Promise<void>
  onError: (sessionId: string, error: unknown) => void
}

export interface SessionUpdateDispatcher {
  dispatch: (payload: RuntimeSessionUpdateEnvelope) => void
  flush: () => void
  dispose: () => void
}

function isBatchableLiveUpdate(payload: RuntimeSessionUpdateEnvelope): boolean {
  return payload.update.type === 'text_delta'
    || payload.update.type === 'reasoning_delta'
    || payload.update.type === 'tool_call_progress'
}

export function createSessionUpdateDispatcher(
  dependencies: SessionUpdateDispatcherDependencies,
): SessionUpdateDispatcher {
  let timer: ReturnType<typeof setTimeout> | null = null
  const pendingBySession = new Map<string, RuntimeSessionUpdateEnvelope[]>()

  function flush() {
    if (timer !== null) clearTimeout(timer)
    timer = null
    const batches = [...pendingBySession.entries()]
    pendingBySession.clear()
    for (const [sessionId, payloads] of batches) {
      try {
        const applied = dependencies.applyLiveBatch(payloads)
        void Promise.resolve(applied).catch((error) => {
          dependencies.onError(sessionId, error)
        })
      } catch (error) {
        dependencies.onError(sessionId, error)
      }
    }
  }

  function dispatch(payload: RuntimeSessionUpdateEnvelope) {
    if (isBatchableLiveUpdate(payload)) {
      const pending = pendingBySession.get(payload.sessionId) ?? []
      pending.push(payload)
      pendingBySession.set(payload.sessionId, pending)
      if (timer === null) timer = setTimeout(flush, LIVE_UPDATE_BATCH_MS)
      return
    }

    flush()
    try {
      void dependencies.processUpdate(payload).catch((error) => {
        dependencies.onError(payload.sessionId, error)
      })
    } catch (error) {
      dependencies.onError(payload.sessionId, error)
    }
  }

  return {
    dispatch,
    flush,
    dispose: () => {
      if (timer !== null) clearTimeout(timer)
      timer = null
      pendingBySession.clear()
    },
  }
}

export function recordCoreBridgeError(sessionId: string | null, error: unknown): void {
  const message = resolveErrorMessage(error)
  if (sessionId) {
    useRuntimeStore.getState().markSyncFailed(sessionId, message)
  } else {
    useSessionStore.setState({ error: message })
  }
}

export function useCoreEventBridge(): void {
  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | null = null
    const processUpdate = (payload: RuntimeSessionUpdateEnvelope) => processSessionUpdate(payload)
    const dispatcher = createSessionUpdateDispatcher({
      processUpdate,
      applyLiveBatch: async (payloads) => {
        const sessionId = payloads[0]?.sessionId
        if (!sessionId) return
        const store = useRuntimeStore.getState()
        const lastSequence = store.bySession[sessionId]?.lastSequence ?? 0
        const contiguous = payloads.every((payload, index) =>
          supportsRuntimeSessionUpdateVersion(payload.version)
          && payload.sequence === lastSequence + index + 1
        )
        if (contiguous) {
          store.applyBatch(payloads)
          return
        }
        for (const payload of payloads) await processUpdate(payload)
      },
      onError: (sessionId, error) => {
        if (!disposed) recordCoreBridgeError(sessionId, error)
      },
    })
    void listenToSessionUpdates((payload) => {
      if (!disposed) dispatcher.dispatch(payload)
    }).then((dispose) => {
      if (disposed) dispose()
      else unlisten = dispose
    }).catch((error) => {
      if (!disposed) recordCoreBridgeError(null, error)
    })
    return () => {
      disposed = true
      dispatcher.dispose()
      unlisten?.()
    }
  }, [])
}
