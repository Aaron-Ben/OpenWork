import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import { AppearanceSettings } from './AppearanceSettings'

describe('AppearanceSettings', () => {
  it('offers explicit light, dark, and system theme choices', () => {
    const markup = renderToStaticMarkup(<AppearanceSettings />)

    expect(markup).toContain('外观')
    expect(markup).toContain('亮色')
    expect(markup).toContain('暗色')
    expect(markup).toContain('跟随系统')
    expect(markup).toContain('简体中文')
    expect(markup).toContain('繁體中文')
    expect(markup).toContain('English')
  })
})
