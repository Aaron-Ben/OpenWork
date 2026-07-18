import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { ContentBlock } from '../../../type/parts'
import { ToolActivityList, collectToolActivities } from './ToolActivityList'

const parts: ContentBlock[] = [
  {
    type: 'tool_call',
    id: 'call-bash',
    name: 'bash',
    input: JSON.stringify({ command: 'cargo test -p openwork-core' }),
    state: 'finished',
  },
  {
    type: 'tool_result',
    id: 'call-bash',
    name: 'bash',
    output: [{ type: 'text', text: '2 passed' }],
    state: 'success',
  },
  {
    type: 'tool_call',
    id: 'call-write',
    name: 'write',
    input: JSON.stringify({ path: '/workspace/src/main.rs', content: 'fn main() {}' }),
    state: 'finished',
  },
]

describe('ToolActivityList', () => {
  it('pairs a tool result with its call instead of rendering a duplicate row', () => {
    const activities = collectToolActivities(parts)

    expect(activities).toHaveLength(2)
    expect(activities[0]).toMatchObject({
      id: 'call-bash',
      name: 'bash',
      summary: 'cargo test -p openwork-core',
      output: '2 passed',
      state: 'success',
    })
    expect(activities[1]).toMatchObject({
      id: 'call-write',
      name: 'write',
      summary: '/workspace/src/main.rs',
    })
  })

  it('renders compact list rows with one collapsible activity summary', () => {
    const markup = renderToStaticMarkup(<ToolActivityList parts={parts} />)

    expect(markup).toContain('data-tool-activity-list="true"')
    expect(markup).toContain('data-tool-activity-summary="true"')
    expect(markup.match(/data-tool-activity-row=/g)).toHaveLength(2)
    expect(markup).toContain('cargo test -p openwork-core')
    expect(markup).toContain('main.rs')
    expect(markup).not.toContain('rounded-lg border border-line bg-paper-hover')
  })

  it('exposes a stable provider tool-call link into the matching trace span', () => {
    const markup = renderToStaticMarkup(
      <ToolActivityList parts={parts} onOpenTrace={() => undefined} />,
    )

    expect(markup).toContain('data-open-tool-trace="call-bash"')
    expect(markup).toContain('data-open-tool-trace="call-write"')
  })
})
