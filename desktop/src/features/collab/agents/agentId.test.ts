import { describe, expect, it } from 'vitest'

import { MAX_AGENT_ID_LENGTH, deriveAgentSlug } from './agentId'

describe('deriveAgentSlug', () => {
  it('lowercases the name', () => {
    expect(deriveAgentSlug('Alice')).toBe('alice')
  })

  it('turns spaces and hyphens into underscores', () => {
    expect(deriveAgentSlug('Code Review')).toBe('code_review')
    expect(deriveAgentSlug('Code-Review')).toBe('code_review')
  })

  it('collapses separator runs and trims the ends', () => {
    expect(deriveAgentSlug('  Code --  Review  ')).toBe('code_review')
  })

  it('drops trailing characters the id cannot hold', () => {
    expect(deriveAgentSlug('Alice!!!')).toBe('alice')
    expect(deriveAgentSlug('Alice -')).toBe('alice')
  })

  it('truncates to the maximum id length', () => {
    const slug = deriveAgentSlug('A'.repeat(120))
    expect(slug).toHaveLength(MAX_AGENT_ID_LENGTH)
    expect(slug).toMatch(/^[a-z][a-z0-9_]*$/)
  })

  it('cannot derive from names without ascii letters', () => {
    expect(deriveAgentSlug('小艾')).toBeNull()
    expect(deriveAgentSlug('小 艾!!')).toBeNull()
    expect(deriveAgentSlug('42')).toBeNull()
    expect(deriveAgentSlug(' ')).toBeNull()
  })

  it('keeps the ascii part of mixed-language names', () => {
    expect(deriveAgentSlug('Alice小艾')).toBe('alice')
    expect(deriveAgentSlug('Code 小 Review')).toBe('code_review')
    expect(deriveAgentSlug('小艾 Alice')).toBe('alice')
  })

  it('skips leading digits and underscores', () => {
    expect(deriveAgentSlug('42 Alice')).toBe('alice')
    expect(deriveAgentSlug('_alice')).toBe('alice')
  })
})
