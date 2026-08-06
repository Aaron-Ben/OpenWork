import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { coreCommands } from './commands'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

describe('trace command bridge', () => {
  beforeEach(() => vi.clearAllMocks())

  it('maps trace-id and payload reads to their Tauri commands', async () => {
    vi.mocked(invoke).mockResolvedValue(null)

    await coreCommands.getTraceById('trace-manual')
    await coreCommands.getSpanPayload('span-summary', 'system_context')

    expect(invoke).toHaveBeenNthCalledWith(1, 'runtime_trace_get_by_id', {
      traceId: 'trace-manual',
    })
    expect(invoke).toHaveBeenNthCalledWith(2, 'runtime_trace_payload_get', {
      spanId: 'span-summary',
      slot: 'system_context',
    })
  })

  it('maps runtime content-policy updates without a restart command', async () => {
    vi.mocked(invoke).mockResolvedValue('off')

    await expect(coreCommands.setTraceContentPolicy('off')).resolves.toBe('off')
    expect(invoke).toHaveBeenCalledWith('runtime_trace_content_policy_set', { policy: 'off' })
  })

  it('maps skill listing to the user-level Tauri command without a project argument', async () => {
    vi.mocked(invoke).mockResolvedValue({ skills: [], warnings: [] })

    await expect(coreCommands.listSkills()).resolves.toEqual({ skills: [], warnings: [] })
    expect(invoke).toHaveBeenCalledWith('list_skills')
  })

  it('maps skill detail reads to the read_skill command with the skill path', async () => {
    vi.mocked(invoke).mockResolvedValue({
      source: 'agents',
      name: 'commit',
      description: 'Create commits.',
      path: '/Users/me/.agents/skills/commit/SKILL.md',
      body: 'Body',
    })

    await expect(
      coreCommands.readSkill('/Users/me/.agents/skills/commit/SKILL.md'),
    ).resolves.toMatchObject({ name: 'commit', body: 'Body' })
    expect(invoke).toHaveBeenCalledWith('read_skill', {
      path: '/Users/me/.agents/skills/commit/SKILL.md',
    })
  })

  it('maps skill status changes by name and returns the refreshed listing', async () => {
    vi.mocked(invoke).mockResolvedValue({ skills: [], warnings: [] })

    await expect(coreCommands.setSkillDisabled('commit', true)).resolves.toEqual({
      skills: [],
      warnings: [],
    })
    expect(invoke).toHaveBeenCalledWith('set_skill_disabled', {
      name: 'commit',
      disabled: true,
    })
  })

  it('submits ordered structured user input', async () => {
    vi.mocked(invoke).mockResolvedValue({ turnId: 'turn-1', clientRequestId: 'request-1' })
    const input = [{
      type: 'skill' as const,
      name: 'commit',
      path: '/Users/me/.agents/skills/commit/SKILL.md',
    }, {
      type: 'text' as const,
      text: 'Use $commit.',
    }]

    await coreCommands.startTurn('session-1', 'request-1', input, 258_000)

    expect(invoke).toHaveBeenCalledWith('runtime_turn_start', {
      sessionId: 'session-1',
      clientRequestId: 'request-1',
      input,
      contextWindowTokens: 258_000,
    })
  })
})
