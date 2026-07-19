import { useEffect } from 'react'

import { listenToSessionUpdates } from '../bridge/events'
import { useRuntimeStore } from '../features/chat/runtimeStore'
import { useSessionStore } from '../features/sessions/sessionStore'
import { resolveErrorMessage } from '../utils/commandError'
import { processSessionUpdate } from './coreEventController'

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
    void listenToSessionUpdates((payload) => {
      if (!disposed) {
        void processSessionUpdate(payload).catch((error) => {
          if (!disposed) recordCoreBridgeError(payload.sessionId, error)
        })
      }
    }).then((dispose) => {
      if (disposed) dispose()
      else unlisten = dispose
    }).catch((error) => {
      if (!disposed) recordCoreBridgeError(null, error)
    })
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])
}
