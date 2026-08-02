import { describe, expect, it } from 'vitest'

import { shouldCollapseSidebar, useNavigationStore } from './navigationStore'

describe('shouldCollapseSidebar', () => {
  it('keeps the main content usable when the desktop window becomes narrow', () => {
    expect(shouldCollapseSidebar(375)).toBe(true)
    expect(shouldCollapseSidebar(719)).toBe(true)
    expect(shouldCollapseSidebar(720)).toBe(false)
    expect(shouldCollapseSidebar(1440)).toBe(false)
  })

  it('keeps a message focus request until the matching chat view consumes it', () => {
    useNavigationStore.setState({ messageFocus: null })

    useNavigationStore.getState().requestMessageFocus('session-1', 'message-7')
    const request = useNavigationStore.getState().messageFocus
    expect(request).toMatchObject({ sessionId: 'session-1', messageId: 'message-7' })

    useNavigationStore.getState().clearMessageFocus((request?.requestId ?? 0) + 1)
    expect(useNavigationStore.getState().messageFocus).not.toBeNull()
    useNavigationStore.getState().clearMessageFocus(request?.requestId ?? 0)
    expect(useNavigationStore.getState().messageFocus).toBeNull()
  })
})
