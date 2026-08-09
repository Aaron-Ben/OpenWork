import { describe, expect, it } from 'vitest'

import {
  shouldCollapseAgentRail,
  shouldCollapseSidebar,
  useNavigationStore,
} from './navigationStore'

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

describe('shouldCollapseAgentRail', () => {
  it('drops the agent rail before the sidebar as the window narrows', () => {
    expect(shouldCollapseAgentRail(1_099)).toBe(true)
    expect(shouldCollapseAgentRail(1_100)).toBe(false)
    expect(shouldCollapseAgentRail(1_440)).toBe(false)
    // 右栏先让位：在这个宽度下右栏已经收起，左栏还留着。
    expect(shouldCollapseAgentRail(900)).toBe(true)
    expect(shouldCollapseSidebar(900)).toBe(false)
  })
})

describe('agent focus', () => {
  it('opens and closes the sub-agent view', () => {
    useNavigationStore.setState({ agentFocus: null })

    useNavigationStore.getState().focusAgent('parent-1', 'child-2')
    expect(useNavigationStore.getState().agentFocus)
      .toEqual({ parentSessionId: 'parent-1', childSessionId: 'child-2' })

    useNavigationStore.getState().clearAgentFocus()
    expect(useNavigationStore.getState().agentFocus).toBeNull()
  })

  it('keeps the same state object when clearing an already empty focus', () => {
    useNavigationStore.setState({ agentFocus: null })
    const before = useNavigationStore.getState()
    useNavigationStore.getState().clearAgentFocus()
    expect(useNavigationStore.getState()).toBe(before)
  })
})
