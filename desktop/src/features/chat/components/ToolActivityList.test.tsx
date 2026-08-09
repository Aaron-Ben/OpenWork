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

const createdFileParts: ContentBlock[] = [
  {
    type: 'tool_call',
    id: 'call-write-created',
    name: 'write',
    input: JSON.stringify({
      path: 'src/config.ts',
      content: [
        'export const config = {',
        "  mode: 'test',",
        '  retries: 3,',
        '  verbose: true,',
        '  timeout: 5000,',
        '  cache: false,',
        '  color: true,',
        '}',
      ].join('\n'),
    }),
    state: 'finished',
  },
  {
    type: 'tool_result',
    id: 'call-write-created',
    name: 'write',
    output: [{ type: 'text', text: 'created 139 bytes to src/config.ts' }],
    state: 'success',
    artifacts: [{
      kind: 'file_change',
      payload: {
        changeId: 'change-write-created',
        path: 'src/config.ts',
        kind: 'created',
        additions: 8,
        deletions: 0,
        beforeHash: null,
        afterHash: 'after-write',
        undone: false,
        hunks: [{
          oldStart: 0,
          oldLines: 0,
          newStart: 1,
          newLines: 8,
          lines: [
            { kind: 'addition', oldLine: null, newLine: 1, content: 'export const config = {' },
            { kind: 'addition', oldLine: null, newLine: 2, content: "  mode: 'test'," },
            { kind: 'addition', oldLine: null, newLine: 3, content: '  retries: 3,' },
            { kind: 'addition', oldLine: null, newLine: 4, content: '  verbose: true,' },
            { kind: 'addition', oldLine: null, newLine: 5, content: '  timeout: 5000,' },
            { kind: 'addition', oldLine: null, newLine: 6, content: '  cache: false,' },
            { kind: 'addition', oldLine: null, newLine: 7, content: '  color: true,' },
            { kind: 'addition', oldLine: null, newLine: 8, content: '}' },
          ],
        }],
      },
    }],
  },
]

const successfulBashParts: ContentBlock[] = [
  {
    type: 'tool_call',
    id: 'call-bash-success',
    name: 'bash',
    input: JSON.stringify({ command: 'node --test src/**tests**' }),
    state: 'finished',
  },
  {
    type: 'tool_result',
    id: 'call-bash-success',
    name: 'bash',
    output: [{
      type: 'text',
      text: 'TAP version 13\nok 1 - timer\n[exit 0; duration 1400 ms]',
    }],
    state: 'success',
  },
]

const failedBashParts: ContentBlock[] = [
  {
    type: 'tool_call',
    id: 'call-bash-failed',
    name: 'bash',
    input: JSON.stringify({ command: 'node --test src/timer.test.ts' }),
    state: 'finished',
  },
  {
    type: 'tool_result',
    id: 'call-bash-failed',
    name: 'bash',
    output: [{
      type: 'text',
      text: 'TAP version 13\n[stderr]\nsrc/timer.test.ts:18: expected 2, received 1\n[exit 1; duration 120 ms]',
    }],
    // bash 的非零退出仍是完成的工具结果，展示层必须读取 footer 才能识别失败。
    state: 'success',
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
  it('does not render update_plan as a generic tool activity', () => {
    const planParts: ContentBlock[] = [{
      type: 'tool_call',
      id: 'plan-call',
      name: 'update_plan',
      input: '{"plan":[{"step":"test","status":"pending"}]}',
      state: 'finished',
    }, {
      type: 'tool_result',
      id: 'plan-call',
      name: 'update_plan',
      output: [{ type: 'text', text: 'Plan updated' }],
      state: 'success',
    }]

    expect(renderToStaticMarkup(<ToolActivityList parts={planParts} />)).toBe('')
  })

  it('pairs a tool result with its call instead of rendering a duplicate row', () => {
    const activities = collectToolActivities(parts)

    expect(activities).toHaveLength(2)
    expect(activities[0]).toMatchObject({
      id: 'call-bash',
      name: 'bash',
      input: { command: 'cargo test -p openwork-core' },
      output: '2 passed',
      state: 'success',
    })
    expect(activities[1]).toMatchObject({
      id: 'call-write',
      name: 'write',
      input: { path: '/workspace/src/main.rs', content: 'fn main() {}' },
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
    expect(markup).toContain('写入')
    expect(markup).toContain('/workspace/src/main.rs')
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
    expect(markup).toContain('aria-expanded="true"')
    expect(markup).toContain('main.rs')
    expect(markup).toContain('+2')
    expect(markup).toContain('-1')
    expect(markup).not.toContain('data-file-change-summary="true"')
    expect(markup).toContain('data-file-change="change-edit"')
  })

  it('shows an edit as an expanded write-tier diff with an inline undo action', () => {
    const markup = renderToStaticMarkup(
      <ToolActivityList
        parts={fileChangeParts}
        onUndoFileChanges={async () => undefined}
      />,
    )

    expect(markup).toContain('data-tool-tier="write"')
    expect(markup).toContain('lucide-pencil')
    expect(markup).toContain('修改')
    expect(markup).toContain('src/main.rs')
    expect(markup).toContain('1 处改动')
    expect(markup).toContain('+2')
    expect(markup).toContain('-1')
    expect(markup).toContain('data-file-change-code="true"')
    expect(markup).toContain('@@ 第 1 行起')
    expect(markup).toContain('data-tool-undo="change-edit"')
  })

  it('shows a single edit as one integrated code card without a nested file header', () => {
    const markup = renderToStaticMarkup(
      <ToolActivityList
        parts={fileChangeParts}
        onUndoFileChanges={async () => undefined}
      />,
    )

    expect(markup.match(/aria-expanded="true"/g)).toHaveLength(1)
    expect(markup.match(/>src\/main\.rs</g)).toHaveLength(1)
    expect(markup).toContain('data-file-change-code="true"')
    expect(markup).toContain('@@ 第 1 行起')
  })

  it('shows a created write as a line-numbered addition diff', () => {
    const markup = renderToStaticMarkup(
      <ToolActivityList
        parts={createdFileParts}
        onUndoFileChanges={async () => undefined}
      />,
    )

    expect(markup).toContain('data-tool-tier="write"')
    expect(markup).toContain('lucide-file-plus-corner')
    expect(markup).not.toContain('lucide-pencil')
    expect(markup).toContain('新建')
    expect(markup).toContain('src/config.ts')
    expect(markup).toContain('+8 行')
    expect(markup).toContain('data-file-change-code="true"')
    expect(markup).toContain('@@ 第 1 行起')
    expect(markup).toContain('data-diff-line-number="1"')
    expect(markup).toContain('data-diff-line-number="8"')
    expect(markup).toContain('bg-status-success-soft text-status-success-ink')
    expect(markup).toContain('export const config')
    expect(markup).toContain('color: true')
    expect(markup).not.toContain('data-write-preview="call-write-created"')
    expect(markup).toContain('data-tool-undo="change-write-created"')
  })

  it('disables undo while the owning turn is still active', () => {
    const markup = renderToStaticMarkup(
      <ToolActivityList
        parts={fileChangeParts}
        turnActive
        onUndoFileChanges={async () => undefined}
      />,
    )

    expect(markup).toContain('data-tool-undo="change-edit"')
    expect(markup).toMatch(/data-tool-undo="change-edit"[^>]*disabled=""/)
    expect(markup).toContain('Turn 结束后可撤销')
  })

  it('shows an explicit empty-file preview for a created empty write', () => {
    const emptyWrite: ContentBlock[] = [{
      type: 'tool_call', id: 'write-empty', name: 'write', state: 'finished',
      input: '{"path":"src/empty.ts","content":""}',
    }, {
      type: 'tool_result', id: 'write-empty', name: 'write', state: 'success',
      output: [{ type: 'text', text: 'created 0 bytes to src/empty.ts' }],
      artifacts: [{
        kind: 'file_change',
        payload: {
          changeId: 'change-write-empty', path: 'src/empty.ts', kind: 'created',
          additions: 0, deletions: 0, beforeHash: null, afterHash: 'after', undone: false,
          hunks: [],
        },
      }],
    }]

    const markup = renderToStaticMarkup(<ToolActivityList parts={emptyWrite} />)

    expect(markup).toContain('data-write-preview="write-empty"')
    expect(markup).toContain('data-write-empty="true"')
    expect(markup).toContain('空文件')
  })

  it('shows the deletion diff when write clears an existing file', () => {
    const clearedWrite: ContentBlock[] = [{
      type: 'tool_call', id: 'write-clear', name: 'write', state: 'finished',
      input: '{"path":"src/old.ts","content":""}',
    }, {
      type: 'tool_result', id: 'write-clear', name: 'write', state: 'success',
      output: [{ type: 'text', text: 'wrote 0 bytes to src/old.ts' }],
      artifacts: [{
        kind: 'file_change',
        payload: {
          changeId: 'change-write-clear', path: 'src/old.ts', kind: 'modified',
          additions: 0, deletions: 2, beforeHash: 'before', afterHash: 'after', undone: false,
          hunks: [{
            oldStart: 1, oldLines: 2, newStart: 1, newLines: 0,
            lines: [
              { kind: 'deletion', oldLine: 1, newLine: null, content: 'export const old = true' },
              { kind: 'deletion', oldLine: 2, newLine: null, content: 'export default old' },
            ],
          }],
        },
      }],
    }]

    const markup = renderToStaticMarkup(<ToolActivityList parts={clearedWrite} />)

    expect(markup).toContain('data-file-change="change-write-clear"')
    expect(markup.match(/aria-expanded="true"/g)).toHaveLength(1)
    expect(markup.match(/>src\/old\.ts</g)).toHaveLength(1)
    expect(markup).toContain('@@ 第 1 行起')
    expect(markup).toContain('export const old = true')
  })

  it('shows a successful bash result expanded with exit code, duration, and output size', () => {
    const markup = renderToStaticMarkup(
      <ToolActivityList parts={successfulBashParts} />,
    )

    expect(markup).toContain('data-tool-tier="readonly"')
    expect(markup).toContain('lucide-square-terminal')
    expect(markup).not.toContain('lucide-check')
    expect(markup).toContain('运行')
    expect(markup).toContain('node --test src/**tests**')
    expect(markup).toContain('退出码 0')
    expect(markup).toContain('1.4s')
    expect(markup).toContain('data-bash-output="call-bash-success"')
    expect(markup).toContain('stdout · 2 行')
    expect(markup).toContain('TAP version 13')
    expect(markup).toContain('ok 1 - timer')
  })

  it('shows a non-zero bash exit as an expanded failure with real follow-up actions', () => {
    const markup = renderToStaticMarkup(
      <ToolActivityList parts={failedBashParts} onOpenTrace={() => undefined} />,
    )

    expect(markup).toContain('data-tool-tier="failure"')
    expect(markup).toContain('退出码 1')
    expect(markup).not.toContain('120ms')
    expect(markup).not.toContain('aria-expanded')
    expect(markup).toContain('bg-status-danger-soft text-status-danger-ink')
    expect(markup).toContain('data-first-error-line="true"')
    expect(markup).toContain('src/timer.test.ts:18: expected 2, received 1')
    expect(markup).toContain('复制错误')
    expect(markup).toContain('检查 Trace')
  })

  it('groups adjacent successful bash calls but keeps a failed call standalone', () => {
    const successful = Array.from({ length: 5 }, (_, index): ContentBlock[] => [{
      type: 'tool_call',
      id: `bash-group-${index}`,
      name: 'bash',
      input: JSON.stringify({ command: `printf ${index}` }),
      state: 'finished',
    }, {
      type: 'tool_result',
      id: `bash-group-${index}`,
      name: 'bash',
      output: [{ type: 'text', text: `${index}\n[exit 0; duration 10 ms]` }],
      state: 'success',
    }]).flat()
    const failed = failedBashParts.map((part) => ({ ...part, id: 'bash-group-failed' }))

    const markup = renderToStaticMarkup(
      <ToolActivityList parts={[...successful, ...failed]} />,
    )

    expect(markup.match(/data-tool-activity-row=/g)).toHaveLength(2)
    expect(markup).toContain('×5')
    expect(markup.match(/data-bash-output="bash-group-/g)).toHaveLength(6)
  })

  it('bounds a bash output over two thousand lines until the user loads it', () => {
    const output = Array.from(
      { length: 2_005 },
      (_, index) => `output-line-${String(index + 1).padStart(4, '0')}`,
    ).join('\n')
    const call: ContentBlock[] = [{
      type: 'tool_call', id: 'bash-long', name: 'bash',
      input: '{"command":"generate-output"}', state: 'finished',
    }, {
      type: 'tool_result', id: 'bash-long', name: 'bash', state: 'success',
      output: [{ type: 'text', text: `${output}\n[exit 0; duration 600 ms]` }],
    }]

    const markup = renderToStaticMarkup(<ToolActivityList parts={call} />)

    expect(markup).toContain('output-line-0001')
    expect(markup).toContain('output-line-0200')
    expect(markup).not.toContain('output-line-0201')
    expect(markup).not.toContain('output-line-1805')
    expect(markup).toContain('output-line-1806')
    expect(markup).toContain('output-line-2005')
    expect(markup).toContain('中间 1605 行已隐藏')
    expect(markup).toContain('加载完整输出')
  })

  it('shows a failed edit as a static expanded failure with next-step actions', () => {
    const failedEdit: ContentBlock[] = [{
      type: 'tool_call', id: 'edit-failed', name: 'edit', state: 'finished',
      input: '{"filePath":"src/main.rs","oldString":"missing","newString":"new"}',
    }, {
      type: 'tool_result', id: 'edit-failed', name: 'edit', state: 'error',
      output: [{ type: 'text', text: 'oldString not found in src/main.rs' }],
    }]

    const markup = renderToStaticMarkup(
      <ToolActivityList parts={failedEdit} onOpenTrace={() => undefined} />,
    )

    expect(markup).toContain('data-tool-tier="failure"')
    expect(markup).not.toContain('aria-expanded')
    expect(markup).toContain('oldString not found in src/main.rs')
    expect(markup).toContain('复制错误')
    expect(markup).toContain('检查 Trace')
  })

  it('groups adjacent writes while preserving every diff and mixed create-overwrite semantics', () => {
    const write = (
      id: string,
      path: string,
      kind: 'created' | 'modified',
      content: string,
    ): ContentBlock[] => [{
      type: 'tool_call', id, name: 'write', state: 'finished',
      input: JSON.stringify({ path, content }),
    }, {
      type: 'tool_result', id, name: 'write', state: 'success',
      output: [{ type: 'text', text: `${kind} ${path}` }],
      artifacts: [{
        kind: 'file_change',
        payload: {
          changeId: `change-${id}`, path, kind,
          additions: 1, deletions: kind === 'created' ? 0 : 1,
          beforeHash: kind === 'created' ? null : 'before', afterHash: 'after', undone: false,
          hunks: [{
            oldStart: 1, oldLines: kind === 'created' ? 0 : 1,
            newStart: 1, newLines: 1,
            lines: [{ kind: 'addition', oldLine: null, newLine: 1, content }],
          }],
        },
      }],
    }]

    const markup = renderToStaticMarkup(
      <ToolActivityList parts={[
        ...write('write-created', 'src/a.ts', 'created', 'export const a = 1'),
        ...write('write-overwritten', 'src/b.ts', 'modified', 'export const b = 2'),
      ]} />,
    )

    expect(markup.match(/data-tool-activity-row=/g)).toHaveLength(1)
    expect(markup).toContain('×2')
    expect(markup).toContain('写入')
    expect(markup).not.toContain('>新建<')
    expect(markup.match(/data-write-diff=/g)).toHaveLength(2)
    expect(markup.match(/data-file-change-code="true"/g)).toHaveLength(2)
    expect(markup).toContain('export const a = 1')
    expect(markup).toContain('export const b = 2')
  })

  it('shows one file heading per cleared write inside a grouped card', () => {
    const clearedWrite = (id: string, path: string, oldLine: string): ContentBlock[] => [{
      type: 'tool_call', id, name: 'write', state: 'finished',
      input: JSON.stringify({ path, content: '' }),
    }, {
      type: 'tool_result', id, name: 'write', state: 'success',
      output: [{ type: 'text', text: `wrote 0 bytes to ${path}` }],
      artifacts: [{
        kind: 'file_change',
        payload: {
          changeId: `change-${id}`, path, kind: 'modified',
          additions: 0, deletions: 1, beforeHash: 'before', afterHash: 'after', undone: false,
          hunks: [{
            oldStart: 1, oldLines: 1, newStart: 1, newLines: 0,
            lines: [{ kind: 'deletion', oldLine: 1, newLine: null, content: oldLine }],
          }],
        },
      }],
    }]

    const markup = renderToStaticMarkup(
      <ToolActivityList parts={[
        ...clearedWrite('clear-a', 'src/a.ts', 'export const a = 1'),
        ...clearedWrite('clear-b', 'src/b.ts', 'export const b = 2'),
      ]} />,
    )

    expect(markup).toContain('×2')
    expect(markup.match(/>src\/a\.ts</g)).toHaveLength(1)
    expect(markup.match(/>src\/b\.ts</g)).toHaveLength(1)
  })

  it('keeps both the executable and trailing arguments visible for a long bash command', () => {
    const command = 'node scripts/run-tests-with-a-very-long-coverage-configuration.ts --reporter junit --output reports/results.xml'
    const call: ContentBlock[] = [{
      type: 'tool_call', id: 'bash-long-command', name: 'bash', state: 'finished',
      input: JSON.stringify({ command }),
    }]

    const markup = renderToStaticMarkup(<ToolActivityList parts={call} />)

    expect(markup).toContain('data-command-prefix="true"')
    expect(markup).toContain('node scripts/run-tests')
    expect(markup).toContain('data-command-suffix="true"')
    expect(markup).toContain('reports/results.xml')
  })

  it('recognizes stderr when bash produced no stdout before the stderr marker', () => {
    const call: ContentBlock[] = [{
      type: 'tool_call', id: 'bash-stderr-only', name: 'bash', state: 'finished',
      input: '{"command":"failing-command"}',
    }, {
      type: 'tool_result', id: 'bash-stderr-only', name: 'bash', state: 'success',
      output: [{ type: 'text', text: '[stderr]\ncommand not found\n[exit 127; duration 15 ms]' }],
    }]

    const markup = renderToStaticMarkup(<ToolActivityList parts={call} />)

    expect(markup).toContain('stdout · 0 行')
    expect(markup).toContain('stderr · 1 行')
    expect(markup).toContain('data-bash-output-line="stderr"')
    expect(markup).toContain('command not found')
  })

  it('keeps exit-zero stderr successful while visually distinguishing the stream', () => {
    const call: ContentBlock[] = [{
      type: 'tool_call', id: 'bash-warning', name: 'bash', state: 'finished',
      input: '{"command":"cargo check"}',
    }, {
      type: 'tool_result', id: 'bash-warning', name: 'bash', state: 'success',
      output: [{ type: 'text', text: '[stderr]\nwarning: unused import\n[exit 0; duration 50 ms]' }],
    }]

    const markup = renderToStaticMarkup(<ToolActivityList parts={call} />)

    expect(markup).toContain('data-tool-tier="readonly"')
    expect(markup).toContain('data-bash-output-line="stderr"')
    expect(markup).toContain('bg-status-warning-soft text-status-warning-ink')
    expect(markup).not.toContain('复制错误')
  })

  it('keeps a grouped edit row running until every edit has finished', () => {
    const runningEdit: ContentBlock = {
      type: 'tool_call', id: 'edit-running', name: 'edit', state: 'submitted',
      input: '{"filePath":"src/next.rs","oldString":"old","newString":"new"}',
    }

    const markup = renderToStaticMarkup(
      <ToolActivityList parts={[...fileChangeParts, runningEdit]} />,
    )

    expect(markup.match(/data-tool-activity-row=/g)).toHaveLength(1)
    expect(markup).toContain('×2')
    expect(markup).toContain('animate-spin')
  })

  it('labels a timed-out bash call without inventing an exit code', () => {
    const call: ContentBlock[] = [{
      type: 'tool_call', id: 'bash-timeout', name: 'bash', state: 'finished',
      input: '{"command":"slow-command","timeoutMs":1000}',
    }, {
      type: 'tool_result', id: 'bash-timeout', name: 'bash', state: 'error',
      output: [{ type: 'text', text: 'partial output\n[timed out after 1000 ms; duration 1010 ms]' }],
    }]

    const markup = renderToStaticMarkup(<ToolActivityList parts={call} />)

    expect(markup).toContain('data-tool-tier="failure"')
    expect(markup).toContain('已超时')
    expect(markup).toContain('1.0s')
    expect(markup).not.toContain('退出码')
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

  it('uses distinct success icons for read, list, glob, and grep', () => {
    const markup = renderToStaticMarkup(<ToolActivityList parts={readonlyParts} />)

    expect(markup).toContain('lucide-file-text')
    expect(markup).toContain('lucide-list-tree')
    expect(markup).toContain('lucide-files')
    expect(markup).toContain('lucide-search')
    expect(markup).not.toContain('lucide-check')
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
