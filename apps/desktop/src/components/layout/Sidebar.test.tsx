import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import { ProjectItem, Sidebar } from './Sidebar'

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
    expect(markup).toContain('项目')
    expect(markup).toContain('OpenWork')
    expect(markup).toContain('aria-label="打开文件夹"')
    expect(markup).not.toContain('aria-label="项目菜单"')
    expect(markup).toContain('设置')
    expect(markup).not.toContain('运行记录')
    expect(markup).not.toContain('data-activity-navigation="true"')
    expect(markup).toContain('data-sidebar-footer="true"')
    expect(markup).toContain('aria-expanded="true"')
    expect(markup).toContain('data-motion-sidebar="true"')
    expect(markup).toContain('h-full')
    expect(markup).toContain('w-[240px]')
    expect(markup).not.toContain('w-[280px]')
    expect(markup).not.toContain('h-screen')
  })

  it('renders project actions without a destructive filesystem action', () => {
    const markup = renderToStaticMarkup(
      <ProjectItem
        project={{ name: 'OpenWork', path: '/Volumes/Code/OpenWork' }}
        active
        expanded
        onSelect={vi.fn()}
        onRemove={vi.fn()}
        onCreateSession={vi.fn()}
      />,
    )

    expect(markup).toContain('data-project-row="true"')
    expect(markup).toContain('aria-expanded="true"')
    expect(markup).toContain('aria-label="OpenWork 项目操作"')
    expect(markup).toContain('aria-label="在 OpenWork 中创建会话"')
    expect(markup).not.toContain('disabled=""')
    expect(markup).not.toContain('删除电脑上的项目')
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
    expect(markup).not.toContain('aria-label="打开文件夹"')
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
    expect(markup).toContain('运行记录')
    expect(markup).toContain('data-activity-navigation="true"')
    expect(markup).toContain('模型配置')
    expect(markup).toContain('外观')
    expect(markup).not.toContain('打开文件夹')
  })

  it('keeps the trace page inside the settings navigation', () => {
    const markup = renderToStaticMarkup(
      <Sidebar
        view="traces"
        expanded
        onToggleExpanded={vi.fn()}
        onNavigate={vi.fn()}
      />,
    )

    expect(markup).toContain('data-settings-sidebar="true"')
    expect(markup).toContain('返回 OpenWork')
    expect(markup).toContain('运行记录')
    expect(markup).toContain('模型配置')
    expect(markup).toContain('外观')
    expect(markup).not.toContain('打开文件夹')
  })
})
