import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import { CollabRail, railTopInsetClass } from './CollabRail'

describe('CollabRail', () => {
  it('reserves a draggable traffic-light inset only on macOS', () => {
    expect(railTopInsetClass(true)).toContain('h-7')
    expect(railTopInsetClass(false)).toBe('hidden')
    const mac = renderToStaticMarkup(
      <CollabRail view="rooms" unreadCount={3} permissionCount={2} macOS />,
    )
    expect(mac).toContain('data-tauri-drag-region="deep"')
  })

  it('renders unread and approvals as distinct badges', () => {
    const html = renderToStaticMarkup(
      <CollabRail view="rooms" unreadCount={3} permissionCount={2} macOS={false} />,
    )
    expect(html).toContain('data-collab-unread="3"')
    expect(html).toContain('data-collab-approvals="2"')
  })
})
