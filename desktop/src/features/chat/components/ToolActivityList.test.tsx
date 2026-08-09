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

const readonlyParts: ContentBlock[] = [
  {
    type: 'tool_call',
    id: 'call-read',
    name: 'read',
    input: JSON.stringify({ path: 'src/timer.mjs', offset: 17, limit: 17 }),
    state: 'finished',
  },
  {
    type: 'tool_result',
    id: 'call-read',
    name: 'read',
    output: [{ type: 'text', text: '    18\tconst timer = setInterval(tick, 1000)\n    19\treturn timer' }],
    state: 'success',
  },
  {
    type: 'tool_call',
    id: 'call-list',
    name: 'list',
    input: JSON.stringify({ path: 'src/' }),
    state: 'finished',
  },
  {
    type: 'tool_result',
    id: 'call-list',
    name: 'list',
    output: [{ type: 'text', text: 'components/\ntimer.mjs\npoll.mjs' }],
    state: 'success',
  },
  {
    type: 'tool_call',
    id: 'call-glob',
    name: 'glob',
    input: JSON.stringify({ pattern: 'src/**/*.mjs', path: '.' }),
    state: 'finished',
  },
  {
    type: 'tool_result',
    id: 'call-glob',
    name: 'glob',
    output: [{ type: 'text', text: 'src/timer.mjs\nsrc/poll.mjs\n' }],
    state: 'success',
  },
  {
    type: 'tool_call',
    id: 'call-grep',
    name: 'grep',
    input: JSON.stringify({ pattern: 'setInterval', path: 'src', glob: '**/*.mjs' }),
    state: 'finished',
  },
  {
    type: 'tool_result',
    id: 'call-grep',
    name: 'grep',
    output: [{
      type: 'text',
      text: 'timer.mjs:18:const timer = setInterval(tick, 1000)\npoll.mjs:7:setInterval(poll, 250)',
    }],
    state: 'success',
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

  it('shows useful collapsed summaries for read, list, glob, and grep without backend metadata', () => {
    const markup = renderToStaticMarkup(<ToolActivityList parts={readonlyParts} />)

    expect(markup).toContain('读取')
    expect(markup).toContain('src/timer.mjs')
    expect(markup).toContain('第 18–19 行')
    expect(markup).toContain('共 19 行')
    expect(markup).toContain('列出')
    expect(markup).toContain('1 个目录')
    expect(markup).toContain('2 个文件')
    expect(markup).toContain('匹配')
    expect(markup).toContain('src/**/*.mjs')
    expect(markup).toContain('搜索')
    expect(markup).toContain('setInterval')
    expect(markup).toContain('于 src/**/*.mjs')
    expect(markup).toContain('2 处 / 2 个文件')
  })

  it('keeps read, list, and glob collapsed while grep opens its grouped matches by default', () => {
    const markup = renderToStaticMarkup(<ToolActivityList parts={readonlyParts} />)

    expect(markup).toContain('data-tool-activity-row="call-read"')
    expect(markup).toContain('data-tool-activity-row="call-list"')
    expect(markup).toContain('data-tool-activity-row="call-glob"')
    expect(markup).not.toContain('data-read-details="call-read"')
    expect(markup).not.toContain('data-list-details="call-list"')
    expect(markup).not.toContain('data-glob-details="call-glob"')
    expect(markup).toContain('data-grep-details="call-grep"')
    expect(markup).toContain('timer.mjs · 1 处')
    expect(markup).toContain('>18<')
    expect(markup).toContain('<mark')
    expect(markup).toContain('>setInterval</mark>')
  })

  it('groups five adjacent reads into one row but breaks the group at model text', () => {
    const read = (id: string, path: string): ContentBlock[] => [{
      type: 'tool_call',
      id,
      name: 'read',
      input: JSON.stringify({ path }),
      state: 'finished',
    }, {
      type: 'tool_result',
      id,
      name: 'read',
      output: [{ type: 'text', text: '     1\tcontent' }],
      state: 'success',
    }]
    const adjacent = Array.from({ length: 5 }, (_, index) => read(`read-${index}`, `src/${index}.ts`)).flat()
    const groupedMarkup = renderToStaticMarkup(<ToolActivityList parts={adjacent} />)
    const splitMarkup = renderToStaticMarkup(
      <ToolActivityList parts={[...adjacent.slice(0, 4), { type: 'text', text: '继续检查' }, ...adjacent.slice(4)]} />,
    )

    expect(groupedMarkup.match(/data-tool-activity-row=/g)).toHaveLength(1)
    expect(groupedMarkup).toContain('×5')
    expect(splitMarkup.match(/data-tool-activity-row=/g)).toHaveLength(2)
  })

  it('keeps failed readonly calls standalone and expanded', () => {
    const failed = readonlyParts.slice(0, 2).map((part) =>
      part.type === 'tool_result'
        ? { ...part, output: [{ type: 'text' as const, text: 'file not found' }], state: 'error' as const }
        : part,
    )
    const succeeding = readonlyParts.slice(0, 2).map((part) => ({
      ...part,
      id: 'call-read-next',
    }))
    const markup = renderToStaticMarkup(
      <ToolActivityList parts={[...failed, ...succeeding]} />,
    )
    const failureOnlyMarkup = renderToStaticMarkup(<ToolActivityList parts={failed} />)

    expect(markup.match(/data-tool-activity-row=/g)).toHaveLength(2)
    expect(markup).toContain('data-tool-tier="failure"')
    expect(markup).toContain('data-read-details="call-read"')
    expect(markup).toContain('file not found')
    expect(failureOnlyMarkup).not.toContain('aria-expanded')
    expect(failureOnlyMarkup).toContain('复制错误')
  })

  it('does not treat backend truncation markers as glob files or claim an exact total', () => {
    const truncated: ContentBlock[] = [{
      type: 'tool_call', id: 'glob-truncated', name: 'glob',
      input: '{"pattern":"**/*.ts","path":"."}', state: 'finished',
    }, {
      type: 'tool_result', id: 'glob-truncated', name: 'glob', state: 'success',
      output: [{ type: 'text', text: 'src/a.ts\nsrc/b.ts\n...[12 行已隐藏]...\nial.ts\nsrc/z.ts' }],
    }]

    const markup = renderToStaticMarkup(<ToolActivityList parts={truncated} />)

    expect(markup).toContain('至少 2 个文件')
    expect(markup).not.toContain('5 个文件')
  })

  it('marks a glob result limit as an incomplete count', () => {
    const limited: ContentBlock[] = [{
      type: 'tool_call', id: 'glob-limited', name: 'glob',
      input: '{"pattern":"**/*.ts","path":".","maxResults":2}', state: 'finished',
    }, {
      type: 'tool_result', id: 'glob-limited', name: 'glob', state: 'success',
      output: [{ type: 'text', text: 'src/a.ts\nsrc/b.ts\n[result limit reached at 2; more matches may exist]\n' }],
    }]

    const markup = renderToStaticMarkup(<ToolActivityList parts={limited} />)

    expect(markup).toContain('至少 2 个文件')
  })

  it('keeps an adjacent readonly group running until every call finishes', () => {
    const calls: ContentBlock[] = [{
      type: 'tool_call', id: 'read-done', name: 'read',
      input: '{"path":"done.ts"}', state: 'finished',
    }, {
      type: 'tool_result', id: 'read-done', name: 'read', state: 'success',
      output: [{ type: 'text', text: '     1\tdone' }],
    }, {
      type: 'tool_call', id: 'read-running', name: 'read',
      input: '{"path":"running.ts"}', state: 'submitted',
    }]

    const markup = renderToStaticMarkup(<ToolActivityList parts={calls} />)

    expect(markup.match(/data-tool-activity-row=/g)).toHaveLength(1)
    expect(markup).toContain('animate-spin')
  })

  it('does not turn an unknown files-with-matches count into zero when grep modes are grouped', () => {
    const calls: ContentBlock[] = [{
      type: 'tool_call', id: 'grep-content', name: 'grep',
      input: '{"pattern":"needle","path":"src"}', state: 'finished',
    }, {
      type: 'tool_result', id: 'grep-content', name: 'grep', state: 'success',
      output: [{ type: 'text', text: 'a.ts:1:needle' }],
    }, {
      type: 'tool_call', id: 'grep-files', name: 'grep',
      input: '{"pattern":"needle","path":"src","outputMode":"files_with_matches"}', state: 'finished',
    }, {
      type: 'tool_result', id: 'grep-files', name: 'grep', state: 'success',
      output: [{ type: 'text', text: 'b.ts' }],
    }]

    const markup = renderToStaticMarkup(<ToolActivityList parts={calls} />)

    expect(markup).toContain('2 个文件')
    expect(markup).not.toContain('1 处 / 2 个文件')
  })

  it('shows the grep path as provided instead of guessing that it is a directory', () => {
    const call: ContentBlock[] = [{
      type: 'tool_call', id: 'grep-file', name: 'grep',
      input: '{"pattern":"name","path":"README.md"}', state: 'finished',
    }, {
      type: 'tool_result', id: 'grep-file', name: 'grep', state: 'success',
      output: [{ type: 'text', text: 'README.md:1:name' }],
    }]

    const markup = renderToStaticMarkup(<ToolActivityList parts={call} />)

    expect(markup).toContain('于 README.md')
    expect(markup).not.toContain('README.md/**')
  })

  it('highlights safe literals without executing arbitrary Rust regex in the browser', () => {
    const caseInsensitive: ContentBlock[] = [{
      type: 'tool_call', id: 'grep-inline-flag', name: 'grep',
      input: '{"pattern":"(?i)needle","path":"src"}', state: 'finished',
    }, {
      type: 'tool_result', id: 'grep-inline-flag', name: 'grep', state: 'success',
      output: [{ type: 'text', text: 'a.ts:1:İNeedle' }],
    }]
    const complex: ContentBlock[] = [{
      type: 'tool_call', id: 'grep-complex', name: 'grep',
      input: '{"pattern":"^needle$","path":"src"}', state: 'finished',
    }, {
      type: 'tool_result', id: 'grep-complex', name: 'grep', state: 'success',
      output: [{ type: 'text', text: 'a.ts:1:needle' }],
    }]

    const literalMarkup = renderToStaticMarkup(<ToolActivityList parts={caseInsensitive} />)
    const complexMarkup = renderToStaticMarkup(<ToolActivityList parts={complex} />)

    expect(literalMarkup).toContain('>Needle</mark>')
    expect(complexMarkup).not.toContain('<mark')
  })

  it('keeps multiple grep files visible when more than fifty matches are truncated', () => {
    const output = [
      ...Array.from({ length: 60 }, (_, index) => `a.ts:${index + 1}:needle`),
      'b.ts:1:needle',
    ].join('\n')
    const call: ContentBlock[] = [{
      type: 'tool_call', id: 'grep-many', name: 'grep',
      input: '{"pattern":"needle","path":"src"}', state: 'finished',
    }, {
      type: 'tool_result', id: 'grep-many', name: 'grep', state: 'success',
      output: [{ type: 'text', text: output }],
    }]

    const markup = renderToStaticMarkup(<ToolActivityList parts={call} />)

    expect(markup).toContain('a.ts · 60 处')
    expect(markup).toContain('b.ts · 1 处')
    expect(markup.match(/class="grid grid-cols-\[3\.5rem_minmax\(0,1fr\)\]"/g)).toHaveLength(50)
    expect(markup).toContain('还有 11 处命中')
  })
})
