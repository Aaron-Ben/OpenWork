import { useEffect } from 'react'

import { runtimeApi } from '../api/runtime'
import { useRuntimeSessionStore } from '../stores/runtimeSessionStore'

export function useRuntimeSessionUpdates() {
  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | null = null
    void runtimeApi.listenToUpdates((payload) => {
      if (!disposed) void useRuntimeSessionStore.getState().ingestUpdate(payload)
    }).then((dispose) => {
      if (disposed) dispose()
      else unlisten = dispose
    })
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])
}
