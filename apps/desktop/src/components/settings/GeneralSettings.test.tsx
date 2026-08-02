import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import { DEFAULT_CONTEXT_WINDOW_TOKENS, useContextWindowStore } from '@/stores/contextWindowStore'
import { useTraceContentStore } from '@/stores/traceContentStore'
import { GeneralSettings } from './GeneralSettings'

describe('GeneralSettings', () => {
  it('renders the merged page header', () => {
    const markup = renderToStaticMarkup(<GeneralSettings />)

    expect(markup).toContain('通用')
    expect(markup).toContain('外观')
    expect(markup).toContain('上下文窗口')
    expect(markup).toContain('Trace 内容')
  })

  it('offers explicit light, dark, and system theme choices plus languages', () => {
    const markup = renderToStaticMarkup(<GeneralSettings />)

    expect(markup).toContain('亮色')
    expect(markup).toContain('暗色')
    expect(markup).toContain('跟随系统')
    expect(markup).toContain('简体中文')
    expect(markup).toContain('繁體中文')
    expect(markup).toContain('English')
  })

  it('renders a model-independent configurable token budget', () => {
    useContextWindowStore.setState({ contextWindowTokens: DEFAULT_CONTEXT_WINDOW_TOKENS })

    const markup = renderToStaticMarkup(<GeneralSettings />)

    expect(markup).toContain('value="258000"')
    expect(markup).toContain('Tokens')
    expect(markup).toContain('不属于某个模型')
    expect(markup).toContain('85%')
    expect(markup).toContain('最多执行一次')
    expect(markup).toContain('/compact')
  })

  it('offers all trace content policies and warns about private source code', () => {
    useTraceContentStore.setState({ policy: 'full', syncState: 'ready', updating: false, error: null })

    const markup = renderToStaticMarkup(<GeneralSettings />)

    expect(markup).toContain('完整')
    expect(markup).toContain('仅压缩')
    expect(markup).toContain('关闭')
    expect(markup).toContain('私有源码内容')
    expect(markup).toContain('无需重启')
  })
})
