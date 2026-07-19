import { describe, expect, it } from 'vitest'

import { shouldCollapseSidebar } from './navigationStore'

describe('shouldCollapseSidebar', () => {
  it('keeps the main content usable when the desktop window becomes narrow', () => {
    expect(shouldCollapseSidebar(375)).toBe(true)
    expect(shouldCollapseSidebar(719)).toBe(true)
    expect(shouldCollapseSidebar(720)).toBe(false)
    expect(shouldCollapseSidebar(1440)).toBe(false)
  })
})
