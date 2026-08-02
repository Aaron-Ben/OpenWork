import { describe, expect, it } from 'vitest'

import { formatBeijingDateTime } from './dateTime'

describe('formatBeijingDateTime', () => {
  it('renders an absolute UTC timestamp in Asia/Shanghai', () => {
    expect(formatBeijingDateTime('2026-07-18T00:30:45.000Z'))
      .toBe('2026-07-18 08:30:45 (Asia/Shanghai)')
  })

  it('returns a placeholder for an invalid timestamp', () => {
    expect(formatBeijingDateTime('not-a-date')).toBe('—')
  })
})
