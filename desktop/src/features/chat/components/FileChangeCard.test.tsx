import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import { FileChangeCard } from './FileChangeCard'
import { FileDiffPanel, type FileChangeView } from './FileDiffPanel'

function change(index: number): FileChangeView {
  return {
    changeId: `change-${index}`,
    path: `src/file-${index}.ts`,
    kind: 'modified',
    additions: index,
    deletions: 0,
    hunks: [],
    beforeHash: `before-${index}`,
    afterHash: `after-${index}`,
    undone: false,
  }
}

describe('FileChangeCard', () => {
  it('shows a compact project-relative summary for workspace changes', () => {
    const workspaceRoot = '/Volumes/Extreme SSD/Code/ProjectTest'
    const changes = ['tetris.js', 'tetris.html', 'tetris.test.mjs', 'tetris.test.mjs', 'index.html']
      .map((name, index) => ({
        ...change(index + 1),
        path: `${workspaceRoot}/${name}`,
        additions: index === 1 || index === 4 ? 0 : index + 1,
        deletions: index === 1 || index === 4 ? 1 : 0,
      }))
    const markup = renderToStaticMarkup(
      <FileChangeCard changes={changes} workspaceRoot={workspaceRoot} />,
    )

    expect(markup).toContain('编辑了 4 个文件')
    expect(markup).toContain('于 ProjectTest')
    expect(markup.match(/data-file-change-row=/g)).toHaveLength(3)
    expect(markup).toContain('data-file-change-path="ProjectTest/tetris.js"')
    expect(markup).not.toContain('data-file-change-path="ProjectTest/index.html"')
    expect(markup).toContain('data-file-change-tone="addition"')
    expect(markup).toContain('data-file-change-tone="deletion"')
    expect(markup).toContain('data-file-change-show-more="true"')
  })

  it('previews three files and offers to reveal the remainder', () => {
    const markup = renderToStaticMarkup(
      <FileChangeCard changes={[1, 2, 3, 4, 5, 6].map(change)} />,
    )

    expect(markup.match(/data-file-change-row=/g)).toHaveLength(3)
    expect(markup).toContain('src/file-1.ts')
    expect(markup).toContain('src/file-3.ts')
    expect(markup).not.toContain('src/file-4.ts')
    expect(markup).not.toContain('src/file-6.ts')
    expect(markup).toContain('data-file-change-show-more="true"')
  })

  it('renders the exact hunk used by an expanded file activity', () => {
    const fileChange = change(1)
    fileChange.hunks = [{
      oldStart: 4,
      oldLines: 1,
      newStart: 4,
      newLines: 1,
      lines: [
        { kind: 'deletion', oldLine: 4, newLine: null, content: 'const version = 2' },
        { kind: 'addition', oldLine: null, newLine: 4, content: 'const version = 3' },
      ],
    }]

    const markup = renderToStaticMarkup(<FileDiffPanel change={fileChange} />)

    expect(markup).toContain('data-file-change="change-1"')
    expect(markup).toContain('@@ -4,1 +4,1 @@')
    expect(markup).toContain('const version = 2')
    expect(markup).toContain('const version = 3')
  })

  it('starts expanded and exposes an accessible code-card toggle', () => {
    const fileChange = change(6)
    fileChange.hunks = [{
      oldStart: 1,
      oldLines: 0,
      newStart: 1,
      newLines: 1,
      lines: [{ kind: 'addition', oldLine: null, newLine: 1, content: 'new line' }],
    }]

    const markup = renderToStaticMarkup(<FileDiffPanel change={fileChange} />)

    expect(markup).toContain('data-file-change-toggle="true"')
    expect(markup).toContain('aria-expanded="true"')
    expect(markup).toContain('aria-controls=')
    expect(markup).toContain('data-file-change-code="true"')
    expect(markup).toContain('收起 src/file-6.ts 的代码差异')
  })

  it('uses one gutter with old numbers for deletions and new numbers otherwise', () => {
    const fileChange = change(2)
    fileChange.hunks = [{
      oldStart: 85,
      oldLines: 2,
      newStart: 85,
      newLines: 2,
      lines: [
        { kind: 'context', oldLine: 85, newLine: 85, content: 'unchanged' },
        { kind: 'deletion', oldLine: 87, newLine: null, content: 'removed' },
        { kind: 'addition', oldLine: null, newLine: 86, content: 'added' },
      ],
    }]

    const markup = renderToStaticMarkup(<FileDiffPanel change={fileChange} />)
    const lineNumbers = Array.from(
      markup.matchAll(/data-diff-line-number="(\d+)"/g),
      (match) => match[1],
    )

    expect(lineNumbers).toEqual(['85', '87', '86'])
  })

  it('offers reapply instead of undo after all changes were undone', () => {
    const undone = change(3)
    undone.undone = true

    const markup = renderToStaticMarkup(
      <FileChangeCard
        changes={[undone]}
        onUndoFileChanges={async () => undefined}
        onReapplyFileChanges={async () => undefined}
      />,
    )

    expect(markup).toContain('data-file-change-reapply="true"')
    expect(markup).toContain('恢复')
    expect(markup).not.toContain('data-file-change-undo="true"')
  })
})
