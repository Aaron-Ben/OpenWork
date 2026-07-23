import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import { DEFAULT_CONTEXT_WINDOW_TOKENS, useContextWindowStore } from '../../stores/contextWindowStore'
import { ContextWindowSettings } from './ContextWindowSettings'

describe('ContextWindowSettings', () => {
  it('renders a model-independent configurable token budget', () => {
    useContextWindowStore.setState({ contextWindowTokens: DEFAULT_CONTEXT_WINDOW_TOKENS })

    const markup = renderToStaticMarkup(<ContextWindowSettings />)

    expect(markup).toContain('上下文窗口')
    expect(markup).toContain('value="258000"')
    expect(markup).toContain('Tokens')
    expect(markup).toContain('不属于某个模型')
    expect(markup).toContain('不会自动压缩')
    expect(markup).toContain('/compact')
  })
})
