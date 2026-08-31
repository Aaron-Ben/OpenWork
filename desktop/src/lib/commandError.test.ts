import { describe, expect, it } from 'vitest'

import { resolveCommandError, resolveErrorMessage } from './commandError'

describe('command error parsing', () => {
  it('preserves a structured Tauri command error', () => {
    const error = resolveCommandError({
      code: 'schema_not_ready',
      message: 'Database schema is not ready',
    })

    expect(error).toEqual({
      code: 'schema_not_ready',
      message: 'Database schema is not ready',
    })
  })

  it('recognizes the session-not-found domain error', () => {
    expect(
      resolveCommandError({
        code: 'session_not_found',
        message: 'Session not found: sess-1',
      }),
    ).toEqual({
      code: 'session_not_found',
      message: 'Session not found: sess-1',
    })
  })

  it('preserves collaboration daemon transport failures instead of masking them', () => {
    expect(
      resolveCommandError({
        code: 'collaboration_unavailable',
        message: 'collaboration daemon rejected the request: log limit must be between 1 and 500',
      }),
    ).toEqual({
      code: 'collaboration_unavailable',
      message: 'collaboration daemon rejected the request: log limit must be between 1 and 500',
    })
  })

  it('normalizes unknown rejections without parsing backend message text', () => {
    expect(resolveCommandError(new Error('network failed'))).toEqual({
      code: 'internal_error',
      message: 'network failed',
    })
    expect(resolveErrorMessage('legacy failure')).toBe('legacy failure')
  })
})
