import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import type { RuntimeSessionUpdateEnvelope } from './compat'

export const SESSION_UPDATE_EVENT = 'openwork://session-update'

export function listenToSessionUpdates(
  handler: (payload: RuntimeSessionUpdateEnvelope) => void,
): Promise<UnlistenFn> {
  return listen<RuntimeSessionUpdateEnvelope>(SESSION_UPDATE_EVENT, (event) => handler(event.payload))
}
