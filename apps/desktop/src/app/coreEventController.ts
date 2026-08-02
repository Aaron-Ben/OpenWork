import { coreCommands } from '@/bridge/commands'
import type {
  RuntimeSessionSnapshot,
  RuntimeSessionUpdateEnvelope,
} from '@/bridge/compat'
import { supportsRuntimeSessionUpdateVersion } from '@/bridge/compat'
import { useRuntimeStore, type RuntimeApplyResult } from '@/features/chat/runtimeStore'
import { useSessionStore } from '@/features/sessions/sessionStore'
import { resolveErrorMessage } from '@/lib/commandError'

export interface CoreEventControllerDependencies {
  apply: (payload: RuntimeSessionUpdateEnvelope) => RuntimeApplyResult
  getLastSequence: (sessionId: string) => number
  markResyncing: (sessionId: string) => void
  markSyncFailed: (sessionId: string, message: string) => void
  replaceSnapshot: (snapshot: RuntimeSessionSnapshot) => void
  replayUpdates: (sessionId: string, afterSequence: number) => Promise<RuntimeSessionUpdateEnvelope[]>
  loadSnapshot: (sessionId: string) => Promise<RuntimeSessionSnapshot>
  reloadCanonical: (sessionId: string) => Promise<boolean>
  reconcileCanonical: (sessionId: string) => void
  refreshSessions: () => Promise<void>
}

function defaultDependencies(): CoreEventControllerDependencies {
  return {
    apply: (payload) => useRuntimeStore.getState().apply(payload),
    getLastSequence: (sessionId) =>
      useRuntimeStore.getState().bySession[sessionId]?.lastSequence ?? 0,
    markResyncing: (sessionId) => useRuntimeStore.getState().markResyncing(sessionId),
    markSyncFailed: (sessionId, message) =>
      useRuntimeStore.getState().markSyncFailed(sessionId, message),
    replaceSnapshot: (snapshot) => useRuntimeStore.getState().replaceSnapshot(snapshot),
    replayUpdates: coreCommands.replayUpdates,
    loadSnapshot: coreCommands.snapshot,
    reloadCanonical: (sessionId) => useSessionStore.getState().reload(sessionId),
    reconcileCanonical: (sessionId) => useRuntimeStore.getState().reconcileCanonical(sessionId),
    refreshSessions: () => useSessionStore.getState().fetchAll(),
  }
}

async function loadSnapshotForResync(
  sessionId: string,
  deps: CoreEventControllerDependencies,
): Promise<RuntimeSessionSnapshot | null> {
  deps.markResyncing(sessionId)
  try {
    const snapshot = await deps.loadSnapshot(sessionId)
    deps.replaceSnapshot(snapshot)
    await deps.reloadCanonical(sessionId)
    return snapshot
  } catch (error) {
    deps.markSyncFailed(sessionId, resolveErrorMessage(error))
    return null
  }
}

export async function resyncSessionView(
  sessionId: string,
  deps: CoreEventControllerDependencies = defaultDependencies(),
): Promise<void> {
  await loadSnapshotForResync(sessionId, deps)
}

export async function processSessionUpdate(
  payload: RuntimeSessionUpdateEnvelope,
  deps: CoreEventControllerDependencies = defaultDependencies(),
): Promise<void> {
  if (!supportsRuntimeSessionUpdateVersion(payload.version)) {
    await loadSnapshotForResync(payload.sessionId, deps)
    deps.markSyncFailed(
      payload.sessionId,
      `Unsupported session update version: ${payload.version}`,
    )
    return
  }

  let lastSequence = deps.getLastSequence(payload.sessionId)
  if (payload.sequence > lastSequence + 1) {
    deps.markResyncing(payload.sessionId)
    try {
      const replay = await deps.replayUpdates(payload.sessionId, lastSequence)
      for (const recovered of replay.sort((left, right) => left.sequence - right.sequence)) {
        if (recovered.sequence > deps.getLastSequence(payload.sessionId)) deps.apply(recovered)
      }
    } catch {
      await loadSnapshotForResync(payload.sessionId, deps)
    }
    lastSequence = deps.getLastSequence(payload.sessionId)
    if (payload.sequence > lastSequence + 1) {
      const snapshot = await loadSnapshotForResync(payload.sessionId, deps)
      lastSequence = snapshot?.lastUpdateSequence ?? deps.getLastSequence(payload.sessionId)
    }
  }

  if (payload.sequence <= lastSequence) return
  const result = deps.apply(payload)
  if (result.sequenceGap || result.duplicate) return

  if (result.draftCleared || result.terminal) {
    const reloaded = await deps.reloadCanonical(payload.sessionId)
    if (reloaded) deps.reconcileCanonical(payload.sessionId)
  }
  if (result.terminal) await deps.refreshSessions()
}
