import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import { headerInsetClass, SidebarReveal } from './SidebarReveal'

describe('headerInsetClass', () => {
  it('clears the macOS window controls only while the sidebar is collapsed', () => {
    expect(headerInsetClass(false, true)).toBe('pl-20 pr-4')
    expect(headerInsetClass(true, true)).toBe('px-4')
  })

  it('keeps symmetric padding on platforms without inset window controls', () => {
    expect(headerInsetClass(false, false)).toBe('px-4')
    expect(headerInsetClass(true, false)).toBe('px-4')
  })
})

describe('SidebarReveal', () => {
  it('offers a way back to the sidebar once it is collapsed', () => {
    const markup = renderToStaticMarkup(
      <SidebarReveal sidebarExpanded={false} onToggleSidebar={vi.fn()} />,
    )

    expect(markup).toContain('aria-label="展开侧栏"')
    expect(markup).toContain('aria-expanded="false"')
  })

  it('renders nothing while the sidebar is already showing', () => {
    const markup = renderToStaticMarkup(
      <SidebarReveal sidebarExpanded onToggleSidebar={vi.fn()} />,
    )

    expect(markup).toBe('')
  })
})
