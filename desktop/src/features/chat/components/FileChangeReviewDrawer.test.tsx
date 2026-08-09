import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import { FileChangeCard } from './FileChangeCard'
import { FileChangeReviewDrawer } from './FileChangeReviewDrawer'
import type { FileChangeView } from './FileDiffPanel'

function change(index: number, additions: number, deletions: number): FileChangeView {
  return {
    changeId: `change-${index}`,
    path: `src/file-${index}.ts`,
    kind: 'modified',
    additions,
    deletions,
    hunks: [],
    beforeHash: `before-${index}`,
    afterHash: `after-${index}`,
    undone: false,
  }
}

function markup(changes: FileChangeView[], workspaceRoot?: string): string {
  return renderToStaticMarkup(
    <FileChangeReviewDrawer
      changes={changes}
      workspaceRoot={workspaceRoot}
      onClose={() => undefined}
    />,
  )
}

/// 只取遮罩和 <aside> 两个开标签。内层大量元素也带 flex-1 之类，整篇匹配会误伤。
function drawerShell(html: string): string {
  return html.slice(0, html.indexOf('<header'))
}

describe('FileChangeReviewDrawer', () => {
  it('与上下文窗口抽屉同一副外壳：遮罩 + 贴右侧铺满纵向', () => {
    const shell = drawerShell(markup([change(1, 3, 0)]))

    expect(shell).toContain('data-file-change-review-drawer="true"')
    expect(shell).toContain('fixed inset-0')
    expect(shell).toContain('backdrop-blur')
    expect(shell).toContain('inset-y-0 right-0')
    // 居中弹窗的特征，不该出现
    expect(shell).not.toContain('place-items-center')
  })

  it('作为对话框暴露可访问的名字，并给出关闭入口', () => {
    const html = markup([change(1, 3, 0)])

    expect(html).toContain('role="dialog"')
    expect(html).toContain('aria-modal="true"')
    expect(html).toContain('aria-label="查看文件修改"')
    expect(html).toContain('data-file-change-review-close="true"')
  })

  it('表头汇总文件数与增删总量', () => {
    const html = markup([change(1, 80, 0), change(2, 206, 6)])

    expect(html).toContain('共 2 个文件')
    expect(html).toContain('+286')
    expect(html).toContain('-6')
  })

  it('逐个文件渲染差异面板', () => {
    const html = markup([change(1, 1, 0), change(2, 2, 0)])

    expect(html).toContain('data-file-change="change-1"')
    expect(html).toContain('data-file-change="change-2"')
    expect(html.match(/data-file-change-toggle=/g)).toHaveLength(2)
  })

  it('按唯一文件汇总重复修改，并默认展开第一个项目相对路径', () => {
    const workspaceRoot = '/Volumes/Extreme SSD/Code/ProjectTest'
    const first = {
      ...change(1, 2, 1),
      path: `${workspaceRoot}/snake.test.mjs`,
      hunks: [{
        oldStart: 10,
        oldLines: 0,
        newStart: 10,
        newLines: 1,
        lines: [{ kind: 'addition' as const, oldLine: null, newLine: 10, content: 'first edit' }],
      }],
    }
    const second = {
      ...change(2, 3, 1),
      path: first.path,
      hunks: [{
        oldStart: 20,
        oldLines: 0,
        newStart: 20,
        newLines: 1,
        lines: [{ kind: 'addition' as const, oldLine: null, newLine: 20, content: 'second edit' }],
      }],
    }
    const third = {
      ...change(3, 5, 0),
      path: `${workspaceRoot}/snake.mjs`,
    }

    const html = markup([first, second, third], workspaceRoot)

    expect(html).toContain('共 2 个文件')
    expect(html.match(/data-file-change-review-group=/g)).toHaveLength(2)
    expect(html).toContain('data-file-change-path="ProjectTest/snake.test.mjs"')
    expect(html).toContain('data-file-change-path="ProjectTest/snake.mjs"')
    expect(html.match(/aria-expanded="true"/g)).toHaveLength(1)
    expect(html.match(/aria-expanded="false"/g)).toHaveLength(1)
    expect(html.match(/aria-label="复制 Diff"/g)).toHaveLength(2)
    expect(html).toContain('first edit')
    expect(html).toContain('second edit')
  })
})

describe('FileChangeCard 的查看入口', () => {
  it('没有接收方时不显示，避免点了没反应', () => {
    const html = renderToStaticMarkup(<FileChangeCard changes={[change(1, 1, 0)]} />)

    expect(html).not.toContain('data-file-change-review="true"')
  })

  it('卡片本身不渲染抽屉 —— 抽屉挂在 ChatPage，与另外两个抽屉同级', () => {
    const html = renderToStaticMarkup(
      <FileChangeCard changes={[change(1, 1, 0)]} onReviewFileChanges={() => undefined} />,
    )

    expect(html).toContain('data-file-change-review="true"')
    expect(html).not.toContain('data-file-change-review-drawer="true"')
  })
})
