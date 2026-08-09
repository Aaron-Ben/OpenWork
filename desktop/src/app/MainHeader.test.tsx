import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import { MainHeader } from './MainHeader'

describe('MainHeader', () => {
  it('shows the active conversation title in a dedicated top bar', () => {
    const markup = renderToStaticMarkup(
      <MainHeader title="分析 model-provider-v1 设计" sidebarExpanded onToggleSidebar={vi.fn()} />,
    )

    expect(markup).toContain('data-main-header="true"')
    expect(markup).toContain('data-tauri-drag-region="deep"')
    expect(markup).toContain('分析 model-provider-v1 设计')
    expect(markup).toContain('h-14')
    expect(markup).not.toContain('aria-label="展开侧栏"')
    expect(markup).not.toContain('aria-label="会话操作"')
  })

  it('prefixes the title with the owning project as a breadcrumb', () => {
    const markup = renderToStaticMarkup(
      <MainHeader
        title="Q3 渠道 ROI 异常复盘"
        projectName="增长分析"
        subtitle="主控 + 3 个子智能体 · 12 步 · 01:48"
        sidebarExpanded
        onToggleSidebar={vi.fn()}
      />,
    )

    expect(markup).toContain('增长分析')
    expect(markup).toContain('Q3 渠道 ROI 异常复盘')
    expect(markup).toContain('data-main-header-subtitle="true"')
    expect(markup).toContain('主控 + 3 个子智能体 · 12 步 · 01:48')
  })

  it('keeps the project breadcrumb out of the settings and activity pages', () => {
    const markup = renderToStaticMarkup(
      <MainHeader
        title="运行记录"
        kind="activity"
        projectName="增长分析"
        sidebarExpanded
        onToggleSidebar={vi.fn()}
      />,
    )

    expect(markup).not.toContain('增长分析')
    expect(markup).toContain('运行记录')
  })

  it('never renders export or pause actions', () => {
    const markup = renderToStaticMarkup(
      <MainHeader
        title="Q3 渠道 ROI 异常复盘"
        projectName="增长分析"
        subtitle="单智能体 · 4 步 · 00:21"
        sidebarExpanded
        onToggleSidebar={vi.fn()}
      />,
    )

    expect(markup).not.toContain('导出')
    expect(markup).not.toContain('暂停任务')
    expect(markup).not.toContain('<button type="submit"')
  })

  it('uses a fallback title when no conversation is active', () => {
    const markup = renderToStaticMarkup(
      <MainHeader title={null} sidebarExpanded={false} onToggleSidebar={vi.fn()} />,
    )

    expect(markup).toContain('新会话')
    expect(markup).toContain('aria-label="展开侧栏"')
  })

  it('keeps session actions out of activity and settings pages', () => {
    for (const kind of ['activity', 'settings'] as const) {
      const markup = renderToStaticMarkup(
        <MainHeader
          title="页面标题"
          kind={kind}
          sidebarExpanded
          onToggleSidebar={vi.fn()}
        />,
      )

      expect(markup).not.toContain('aria-label="会话操作"')
    }
  })
})
