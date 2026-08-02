import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import type { RuntimeSessionUpdateEnvelope } from './compat'

export const SESSION_UPDATE_EVENT = 'openwork://session-update'
export const SESSION_UPDATE_BATCH_EVENT = 'openwork://session-update-batch'

export async function listenToSessionUpdates(
  handler: (payload: RuntimeSessionUpdateEnvelope) => void,
): Promise<UnlistenFn> {
  const [unlistenSingle, unlistenBatch] = await Promise.all([
    listen<RuntimeSessionUpdateEnvelope>(SESSION_UPDATE_EVENT, (event) => {
      handler(event.payload)
    }),
    listen<RuntimeSessionUpdateEnvelope[]>(SESSION_UPDATE_BATCH_EVENT, (event) => {
      for (const payload of event.payload) handler(payload)
    }),
  ])
  return () => {
    unlistenSingle()
    unlistenBatch()
  }
}
