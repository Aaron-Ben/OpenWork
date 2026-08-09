import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import { TracePage } from './TracePage'

describe('TracePage', () => {
  it('puts search and the full status dropdown first without introductory controls', () => {
    const markup = renderToStaticMarkup(<TracePage />)

    expect(markup).not.toContain('<h1')
    expect(markup).not.toContain('按会话查看模型调用、工具调用、耗时与错误。正文按需加载。')
    expect(markup).not.toContain('刷新')
    expect(markup).toContain('data-trace-dashboard-summary="true"')
    expect(markup).toContain('搜索会话、目录、模型、Trace 或 Turn ID')
    expect(markup.indexOf('搜索会话、目录、模型、Trace 或 Turn ID'))
      .toBeLessThan(markup.indexOf('data-trace-dashboard-summary="true"'))
    expect(markup).toContain('role="combobox"')
    expect(markup).toContain('aria-label="运行状态"')
    expect(markup).not.toContain('data-trace-status-filter=')
  })
})
