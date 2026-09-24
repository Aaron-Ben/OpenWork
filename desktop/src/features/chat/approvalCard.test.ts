import { describe, expect, it } from 'vitest'

import type { RuntimeApprovalCard } from '@/bridge/compat'
import { approvalKind, approvalPathLabel, commandSegments, primaryDecision } from './approvalCard'

const base: RuntimeApprovalCard = {
  mode: 'auto',
  command: 'rm -rf build',
  justification: null,
  paths: [],
  danger: null,
  previousDenial: null,
}

describe('approvalCard', () => {
  it('classifies the card and defaults to deny whenever a bulk delete is involved', () => {
    const danger = { key: 'rm_recursive_or_force' as const, start: 0, end: 12 }
    const path = { path: '/w/build', access: 'write' as const, scope: 'subtree' as const, tier: 'normal' as const, inWorkspace: true }

    expect(approvalKind({ ...base, paths: [path] })).toBe('escalation')
    expect(primaryDecision({ ...base, paths: [path] })).toBe('allow_once')
    expect(approvalKind({ ...base, danger })).toBe('danger')
    expect(approvalKind({ ...base, danger, paths: [path] })).toBe('combined')
    expect(primaryDecision({ ...base, danger, paths: [path] })).toBe('deny')
  })

  it('splits the command at UTF-16 offsets and ignores offsets outside the command', () => {
    const command = 'echo 你好 && rm -rf x'
    const start = command.indexOf('rm')

    expect(commandSegments(command, { key: 'rm_recursive_or_force', start, end: command.length })).toEqual([
      { text: 'echo 你好 && ', highlighted: false },
      { text: 'rm -rf x', highlighted: true },
    ])
    expect(commandSegments(command, { key: 'rm_recursive_or_force', start: 0, end: 999 })).toEqual([
      { text: command, highlighted: false },
    ])
  })

  it('names workspace paths by the workspace folder and leaves other paths absolute', () => {
    expect(approvalPathLabel('/repo/OpenWork/.git', '/repo/OpenWork/')).toBe('OpenWork/.git')
    expect(approvalPathLabel('/repo/OpenWork', '/repo/OpenWork')).toBe('OpenWork')
    expect(approvalPathLabel('/repo/OpenWorkOther/x', '/repo/OpenWork')).toBe('/repo/OpenWorkOther/x')
    expect(approvalPathLabel('/Users/me/.ssh', undefined)).toBe('/Users/me/.ssh')
  })
})
