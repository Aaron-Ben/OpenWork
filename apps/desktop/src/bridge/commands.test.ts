import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { coreCommands } from './commands'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

describe('trace command bridge', () => {
  beforeEach(() => vi.clearAllMocks())

  it('maps trace-id and payload reads to their Tauri commands', async () => {
    vi.mocked(invoke).mockResolvedValue(null)

    await coreCommands.getTraceById('trace-manual')
    await coreCommands.getSpanPayload('span-summary', 'system_context')

    expect(invoke).toHaveBeenNthCalledWith(1, 'runtime_trace_get_by_id', {
      traceId: 'trace-manual',
    })
    expect(invoke).toHaveBeenNthCalledWith(2, 'runtime_trace_payload_get', {
      spanId: 'span-summary',
      slot: 'system_context',
    })
  })

  it('maps runtime content-policy updates without a restart command', async () => {
    vi.mocked(invoke).mockResolvedValue('off')

    await expect(coreCommands.setTraceContentPolicy('off')).resolves.toBe('off')
    expect(invoke).toHaveBeenCalledWith('runtime_trace_content_policy_set', { policy: 'off' })
  })
})
