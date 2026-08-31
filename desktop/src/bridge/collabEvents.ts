import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export const COLLAB_INVALIDATION_EVENT = 'openwork://collaboration-invalidation'

export type CollabInvalidationKind =
  | 'runtime_ready'
  | 'agent_config'
  | 'room'
  | 'message'
  | 'board'
  | 'engine_inventory'
  | 'runner_status'

export interface CollabInvalidation {
  id: string
  kind: CollabInvalidationKind
  subjectId: string | null
  revision: number | null
  publishedAt: number
}

export function listenToCollabInvalidations(
  handler: (invalidation: CollabInvalidation) => void,
): Promise<UnlistenFn> {
  return listen<CollabInvalidation>(COLLAB_INVALIDATION_EVENT, (event) => {
    handler(event.payload)
  })
}
