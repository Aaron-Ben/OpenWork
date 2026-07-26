import { beforeEach, describe, expect, it, vi } from 'vitest'

import { coreCommands } from '../bridge/commands'
import {
  DEFAULT_TRACE_CONTENT_POLICY,
  TRACE_CONTENT_POLICY_STORAGE_KEY,
  parseTraceContentPolicy,
  useTraceContentStore,
} from './traceContentStore'

vi.mock('../bridge/commands', () => ({
  coreCommands: { setTraceContentPolicy: vi.fn() },
}))

describe('traceContentStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useTraceContentStore.setState({
      policy: DEFAULT_TRACE_CONTENT_POLICY,
      syncState: 'idle',
      updating: false,
      error: null,
    })
  })

  it('rejects unknown persisted policy values', () => {
    expect(parseTraceContentPolicy('full')).toBe('full')
    expect(parseTraceContentPolicy('compaction_only')).toBe('compaction_only')
    expect(parseTraceContentPolicy('disabled')).toBeNull()
  })

  it('pushes the persisted policy to Core before becoming ready', async () => {
    let apply: ((policy: 'off') => void) | undefined
    vi.mocked(coreCommands.setTraceContentPolicy).mockImplementation(() => new Promise((resolve) => {
      apply = resolve as (policy: 'off') => void
    }))
    useTraceContentStore.setState({ policy: 'off' })

    const initializing = useTraceContentStore.getState().initialize()
    expect(useTraceContentStore.getState().syncState).toBe('syncing')
    expect(coreCommands.setTraceContentPolicy).toHaveBeenCalledWith('off')
    apply?.('off')
    await initializing

    expect(useTraceContentStore.getState().syncState).toBe('ready')
    expect(useTraceContentStore.getState().policy).toBe('off')
  })

  it('persists only a policy Core has applied successfully', async () => {
    const setItem = vi.fn()
    vi.stubGlobal('localStorage', { setItem })
    vi.mocked(coreCommands.setTraceContentPolicy).mockResolvedValue('compaction_only')
    useTraceContentStore.setState({ syncState: 'ready' })

    expect(await useTraceContentStore.getState().updatePolicy('compaction_only')).toBe(true)
    expect(setItem).toHaveBeenCalledWith(TRACE_CONTENT_POLICY_STORAGE_KEY, 'compaction_only')
    expect(useTraceContentStore.getState().policy).toBe('compaction_only')
    vi.unstubAllGlobals()
  })
})
