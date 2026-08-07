import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import { CopyButton } from './CopyButton'

describe('CopyButton', () => {
  it('renders an accessible label rather than an icon alone', () => {
    const markup = renderToStaticMarkup(<CopyButton text="hello" />)

    expect(markup).toContain('data-copy-button="true"')
    expect(markup).toContain('aria-label="复制"')
    expect(markup).toContain('title="复制"')
  })

  it('renders nothing when there is nothing worth copying', () => {
    expect(renderToStaticMarkup(<CopyButton text="" />)).toBe('')
    // 必须用表达式：JSX 属性里的 "\n" 是反斜杠加 n 两个字符，不是换行。
    expect(renderToStaticMarkup(<CopyButton text={'   \n  '} />)).toBe('')
  })

  it('keeps caller layout classes alongside its own', () => {
    const markup = renderToStaticMarkup(
      <CopyButton text="hello" className="opacity-0 group-hover:opacity-100" />,
    )

    expect(markup).toContain('opacity-0')
    expect(markup).toContain('group-hover:opacity-100')
    expect(markup).toContain('rounded-md')
  })
})
