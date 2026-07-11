import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import { Sidebar } from './Sidebar'

describe('Sidebar', () => {
  it('renders the expanded OpenWork navigation hierarchy', () => {
    const markup = renderToStaticMarkup(
      <Sidebar
        view="chat"
        expanded
        onToggleExpanded={vi.fn()}
        onNavigate={vi.fn()}
      />,
    )

    expect(markup).toContain('OpenWork')
    expect(markup).toContain('创建会话')
    expect(markup).toContain('会话')
    expect(markup).toContain('设置')
    expect(markup).toContain('data-sidebar-footer="true"')
    expect(markup).toContain('aria-expanded="true"')
    expect(markup).toContain('data-motion-sidebar="true"')
  })

  it('fully hides its navigation when collapsed', () => {
    const markup = renderToStaticMarkup(
      <Sidebar
        view="chat"
        expanded={false}
        onToggleExpanded={vi.fn()}
        onNavigate={vi.fn()}
      />,
    )

    expect(markup).toContain('aria-hidden="true"')
    expect(markup).not.toContain('OpenWork')
    expect(markup).not.toContain('aria-label="创建会话"')
    expect(markup).not.toContain('aria-label="设置"')
  })

  it('replaces conversation navigation with the settings navigation', () => {
    const markup = renderToStaticMarkup(
      <Sidebar
        view="settings-models"
        expanded
        onToggleExpanded={vi.fn()}
        onNavigate={vi.fn()}
      />,
    )

    expect(markup).toContain('data-settings-sidebar="true"')
    expect(markup).toContain('返回 OpenWork')
    expect(markup).toContain('模型配置')
    expect(markup).toContain('外观')
    expect(markup).not.toContain('创建会话')
  })
})
