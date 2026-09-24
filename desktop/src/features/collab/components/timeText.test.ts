import { describe, expect, it } from 'vitest'

import { agoText } from './timeText'

describe('agoText', () => {
  it('says just now, then minutes, hours and days ago', () => {
    expect(agoText(30)).toEqual({ key: 'collab.ago.justNow', values: {} })
    expect(agoText(125)).toEqual({ key: 'collab.ago.minutes', values: { count: 2 } })
    expect(agoText(3 * 3_600 + 5)).toEqual({ key: 'collab.ago.hours', values: { count: 3 } })
    expect(agoText(2 * 86_400)).toEqual({ key: 'collab.ago.days', values: { count: 2 } })
  })
})
