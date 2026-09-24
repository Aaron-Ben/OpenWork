import { describe, expect, it } from 'vitest'

import { identityClasses, identityIndex } from './agentIdentity'

describe('identityIndex', () => {
  it('gives each Agent id the same colour every time, independent of list order', () => {
    const ids = ['ada', 'bo', 'cy', 'dee', 'eve']
    const first = ids.map(identityIndex)
    const reversed = [...ids].reverse().map(identityIndex).reverse()
    expect(reversed).toEqual(first)
    expect(identityIndex('ada')).toBe(identityIndex('ada'))
  })

  it('stays within the six tones and uses all of them across many ids', () => {
    const tones = new Set(Array.from({ length: 60 }, (_, index) => identityIndex(`agent-${index}`)))
    expect([...tones].sort()).toEqual([0, 1, 2, 3, 4, 5])
  })
})

describe('identityClasses', () => {
  it('maps a tone to its static utility classes', () => {
    const classes = identityClasses('ada')
    const tone = identityIndex('ada') + 1
    expect(classes).toEqual({
      text: `text-agent-${tone}`,
      avatar: `bg-agent-${tone}/15 text-agent-${tone}`,
      ring: `ring-agent-${tone}`,
      mention: `bg-agent-${tone}/15 text-agent-${tone}`,
    })
  })
})
