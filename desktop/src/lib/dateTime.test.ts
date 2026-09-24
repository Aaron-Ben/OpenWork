import { describe, expect, it } from 'vitest'

import { beijingDayStamp, elapsedSeconds, formatBeijingClock, formatBeijingDateTime } from './dateTime'

describe('formatBeijingDateTime', () => {
  it('renders an absolute UTC timestamp in Asia/Shanghai', () => {
    expect(formatBeijingDateTime('2026-07-18T00:30:45.000Z'))
      .toBe('2026-07-18 08:30:45 (Asia/Shanghai)')
  })

  it('returns a placeholder for an invalid timestamp', () => {
    expect(formatBeijingDateTime('not-a-date')).toBe('—')
  })
})

describe('formatBeijingClock', () => {
  it('renders hours and minutes in Asia/Shanghai', () => {
    expect(formatBeijingClock('2026-09-25T02:07:59Z')).toBe('10:07')
    expect(formatBeijingClock('2026-09-25T10:07:00+08:00')).toBe('10:07')
  })

  it('returns a placeholder for an invalid timestamp', () => {
    expect(formatBeijingClock('bad')).toBe('—')
  })
})

describe('beijingDayStamp', () => {
  const now = Date.parse('2026-09-25T09:00:00+08:00')

  it('shows the clock for today, a marker for yesterday, and the date before that', () => {
    expect(beijingDayStamp('2026-09-25T00:10:00+08:00', now)).toEqual({ kind: 'today', clock: '00:10' })
    expect(beijingDayStamp('2026-09-24T23:59:00+08:00', now)).toEqual({ kind: 'yesterday' })
    expect(beijingDayStamp('2026-09-23T15:00:00+08:00', now)).toEqual({ kind: 'date', month: 9, day: 23 })
  })

  it('treats an invalid timestamp as no stamp', () => {
    expect(beijingDayStamp('bad', now)).toBeNull()
  })
})

describe('elapsedSeconds', () => {
  it('counts whole seconds since a start and never goes negative', () => {
    const now = Date.parse('2026-09-25T10:10:12+08:00')
    expect(elapsedSeconds('2026-09-25T10:07:00+08:00', now)).toBe(192)
    expect(elapsedSeconds('2026-09-25T10:20:00+08:00', now)).toBe(0)
    expect(elapsedSeconds('bad', now)).toBe(0)
  })
})
