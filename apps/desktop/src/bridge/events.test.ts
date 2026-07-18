import { describe, expect, it } from 'vitest'

import { SESSION_UPDATE_EVENT } from './events'

describe('Core event contract', () => {
  it('matches the process-level event name emitted by the Tauri host', () => {
    expect(SESSION_UPDATE_EVENT).toBe('openwork://session-update')
  })
})
