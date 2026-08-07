import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { ContentBlock } from '@/types/parts'
import {
  ToolActivityList,
  collectFileChanges,
  collectToolActivities,
} from './ToolActivityList'

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

const fileChangeParts: ContentBlock[] = [
  {
    type: 'tool_call',
    id: 'call-edit',
    name: 'edit',
    input: JSON.stringify({ filePath: 'src/main.rs', oldString: 'old', newString: 'new' }),
    state: 'finished',
  },
  {
    type: 'tool_result',
    id: 'call-edit',
    name: 'edit',
    output: [{ type: 'text', text: 'edited src/main.rs' }],
    state: 'success',
    artifacts: [{
      kind: 'file_change',
      payload: {
        changeId: 'change-edit',
        path: 'src/main.rs',
        kind: 'modified',
        additions: 2,
        deletions: 1,
        beforeHash: 'before',
        afterHash: 'after',
        undone: false,
        hunks: [{
          oldStart: 1,
          oldLines: 2,
          newStart: 1,
          newLines: 3,
          lines: [
            { kind: 'context', oldLine: 1, newLine: 1, content: 'fn main() {' },
            { kind: 'deletion', oldLine: 2, newLine: null, content: 'old' },
            { kind: 'addition', oldLine: null, newLine: 2, content: 'new' },
            { kind: 'addition', oldLine: null, newLine: 3, content: '}' },
          ],
        }],
      },
    }],
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

  it('renders compact list rows without a collapsible summary', () => {
    const markup = renderToStaticMarkup(<ToolActivityList parts={parts} />)

    expect(markup).toContain('data-tool-activity-list="true"')
    expect(markup.match(/data-tool-activity-row=/g)).toHaveLength(2)
    expect(markup).toContain('cargo test -p openwork-core')
    expect(markup).toContain('main.rs')
    expect(markup).not.toContain('rounded-lg border border-line bg-paper-hover')
  })

  it('renders a live write row before a file-change artifact exists', () => {
    const markup = renderToStaticMarkup(
      <ToolActivityList parts={[parts[2]]} fileChangePresentation="activity" />,
    )

    expect(markup).toContain('data-tool-activity-row="call-write"')
    expect(markup).toContain('写入 main.rs')
    expect(markup).not.toContain('data-file-change-summary="true"')
  })

  it('exposes a stable provider tool-call link into the matching trace span', () => {
    const markup = renderToStaticMarkup(
      <ToolActivityList parts={parts} onOpenTrace={() => undefined} />,
    )

    expect(markup).toContain('data-open-tool-trace="call-bash"')
    expect(markup).toContain('data-open-tool-trace="call-write"')
  })

  it('renders structured file changes with totals and review controls', () => {
    const activities = collectToolActivities(fileChangeParts)
    const changes = collectFileChanges(activities)
    const markup = renderToStaticMarkup(
      <ToolActivityList
        parts={fileChangeParts}
        fileChangePresentation="summary"
        onUndoFileChanges={async () => undefined}
        onReviewFileChanges={() => undefined}
      />,
    )

    expect(changes).toHaveLength(1)
    expect(changes[0]).toMatchObject({
      changeId: 'change-edit',
      additions: 2,
      deletions: 1,
    })
    expect(markup).toContain('data-file-change-summary="true"')
    expect(markup).toContain('src/main.rs')
    expect(markup).toContain('+2')
    expect(markup).toContain('-1')
    expect(markup).toContain('data-file-change-review="true"')
    expect(markup).toContain('data-file-change-undo="true"')
  })

  it('keeps file changes in the normal expandable tool list while a turn is active', () => {
    const markup = renderToStaticMarkup(
      <ToolActivityList
        parts={[...fileChangeParts, ...parts.slice(0, 2)]}
        fileChangePresentation="activity"
      />,
    )

    expect(markup.match(/data-tool-activity-row=/g)).toHaveLength(2)
    expect(markup).toContain('data-file-change-activity="change-edit"')
    expect(markup).toContain('aria-expanded="false"')
    expect(markup).toContain('main.rs')
    expect(markup).toContain('+2')
    expect(markup).toContain('-1')
    expect(markup).not.toContain('data-file-change-summary="true"')
    expect(markup).not.toContain('data-file-change="change-edit"')
  })
})
