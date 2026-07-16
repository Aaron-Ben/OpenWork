import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))

import { sessionsApi } from './sessions'

describe('sessions trace API', () => {
  beforeEach(() => mocks.invoke.mockReset())

  it('sends the complete filter as one server-side query before pagination', async () => {
    mocks.invoke.mockResolvedValue({ items: [], nextOffset: null })

    await sessionsApi.traceList({
      limit: 25,
      offset: 50,
      query: 'cargo test',
      status: 'failed',
      hasRetry: true,
    })

    expect(mocks.invoke).toHaveBeenCalledWith('trace_list', {
      input: {
        limit: 25,
        offset: 50,
        query: 'cargo test',
        status: 'failed',
        hasRetry: true,
      },
    })
  })

  it('loads one typed span detail independently from the turn tree', async () => {
    mocks.invoke.mockResolvedValue({})

    await sessionsApi.traceSpanDetail('turn-1', 'tool-1')

    expect(mocks.invoke).toHaveBeenCalledWith('trace_span_detail', {
      turnId: 'turn-1',
      spanId: 'tool-1',
    })
  })
})
