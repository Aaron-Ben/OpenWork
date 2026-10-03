import { describe, expect, it } from 'vitest'

import { resolveCommandError, resolveErrorMessage } from './commandError'

describe('command error parsing', () => {
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
