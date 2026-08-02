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
    expect(markup).toContain('h-12')
    expect(markup).not.toContain('aria-label="展开侧栏"')
    expect(markup).not.toContain('aria-label="会话操作"')
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
