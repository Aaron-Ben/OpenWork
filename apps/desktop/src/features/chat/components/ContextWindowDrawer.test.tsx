import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { RuntimeContextWindowInspection } from '@/bridge/compat'
import { ContextWindowDrawer } from './ContextWindowDrawer'

const inspection: RuntimeContextWindowInspection = {
  schemaVersion: 2,
  sessionId: 'session-1',
  currentTurnId: 'turn-2',
  resolvedModelName: 'example-model',
  systemContext: [
    { sourceKey: 'core/agent-system', content: [{ type: 'text', text: 'Base instructions' }] },
    { sourceKey: 'project/AGENTS.md', content: [{ type: 'text', text: 'Project instructions' }] },
  ],
  conversation: [
    {
      messageId: 'message-1',
      turnId: 'turn-1',
      role: 'user',
      content: [{ type: 'text', text: 'Earlier question' }],
    },
    {
      messageId: 'message-2',
      turnId: 'turn-2',
      role: 'assistant',
      content: [{ type: 'text', text: 'Current answer' }],
    },
    {
      messageId: 'message-3',
      turnId: 'turn-2',
      role: 'tool',
      content: [
        { type: 'tool_call', id: 'call-1', name: 'write_file', input: '{"path":"a.ts","content":"const a = 1\\nconst b = 2"}', state: 'finished' },
        {
          type: 'tool_result',
          id: 'call-1',
          name: 'write_file',
          output: [{ type: 'text', text: 'File written' }],
          state: 'success',
        },
      ],
    },
  ],
  toolSurface: [
    {
      name: 'read_file',
      description: 'Read a workspace file.',
      parameters: { type: 'object', properties: { path: { type: 'string' } } },
    },
  ],
  budget: {
    systemContextTokens: 100,
    conversationTokens: 200,
    toolSurfaceTokens: 50,
    estimatedInputTokens: 350,
    reservedOutputTokens: null,
    autoCompactionThresholdPercent: 85,
  },
}

describe('ContextWindowDrawer', () => {
  it('shows the three input regions and marks messages from the current turn', () => {
    const markup = renderToStaticMarkup(
      <ContextWindowDrawer
        sessionId="session-1"
        inspection={inspection}
        contextWindowTokens={1_000}
        highlightedTurnId="turn-2"
        loading={false}
        error={null}
        onRefresh={vi.fn()}
        onClose={vi.fn()}
      />,
    )

    expect(markup).toContain('上下文窗口内容')
    expect(markup).toContain('core/agent-system')
    expect(markup).toContain('project/AGENTS.md')
    expect(markup).toContain('Earlier question')
    expect(markup).toContain('Current answer')
    expect(markup).toContain('data-current-turn="true"')
    expect(markup).toContain('本轮')
    expect(markup).toContain('read_file')
    expect(markup).toContain('350 / 1k')
    expect(markup).toContain('35%')
    expect(markup).toContain('85%')
    // tool blocks render with their name, state badge, and payload
    expect(markup).toContain('write_file')
    expect(markup).toContain('success')
    expect(markup).toContain('File written')
    // JSON tool payloads render entry-by-entry; long strings become code blocks
    expect(markup).toContain('a.ts')
    expect(markup).toContain('const a = 1')
    expect(markup).toContain('>ts<')
    // conversation entries are collapsed by default; only the first system part is open
    expect(markup.match(/<details open/g)).toHaveLength(1)
    // a divider separates messages from different turns (turn-1 → turn-2)
    expect(markup.match(/role="separator"/g)).toHaveLength(1)
  })
})
