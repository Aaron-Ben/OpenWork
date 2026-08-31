import { beforeEach, describe, expect, it, vi } from 'vitest'

const listenMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }))

import {
  COLLAB_INVALIDATION_EVENT,
  listenToCollabInvalidations,
} from './collabEvents'

describe('R4 collaboration invalidation bridge', () => {
  beforeEach(() => listenMock.mockReset())

  it('forwards the Tauri collaboration event payload', async () => {
    const unlisten = vi.fn()
    let listener: ((event: { payload: unknown }) => void) | undefined
    listenMock.mockImplementation(async (_name, registered) => {
      listener = registered
      return unlisten
    })
    const handler = vi.fn()
    const stop = await listenToCollabInvalidations(handler)
    expect(listenMock).toHaveBeenCalledWith(COLLAB_INVALIDATION_EVENT, expect.any(Function))
    const payload = {
      id: 'event-1',
      kind: 'runner_status',
      subjectId: null,
      revision: null,
      publishedAt: 42,
    }

    listener?.({ payload })
    expect(handler).toHaveBeenCalledWith(payload)
    stop()
    expect(unlisten).toHaveBeenCalledOnce()
  })
})
