import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import { useTraceContentStore } from '../../stores/traceContentStore'
import { TraceContentSettings } from './TraceContentSettings'

describe('TraceContentSettings', () => {
  it('offers all policies and warns that Trace can contain private source code', () => {
    useTraceContentStore.setState({ policy: 'full', syncState: 'ready', updating: false, error: null })

    const markup = renderToStaticMarkup(<TraceContentSettings />)

    expect(markup).toContain('Trace 内容')
    expect(markup).toContain('完整')
    expect(markup).toContain('仅压缩')
    expect(markup).toContain('关闭')
    expect(markup).toContain('私有源码内容')
    expect(markup).toContain('无需重启')
  })
})
