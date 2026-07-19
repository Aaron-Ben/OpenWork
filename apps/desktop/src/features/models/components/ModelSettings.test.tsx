import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

const providerStoreState = vi.hoisted(() => ({
  providers: [{
    id: 'provider-1',
    name: 'DeepSeek',
    baseUrl: 'https://api.deepseek.com',
    kind: 'deepseek',
    models: [{ modelId: 'deepseek-chat', modelTier: 'plus', enabled: true }],
    enabled: true,
  }],
  presets: [],
  error: 'provider unavailable',
  remove: async () => undefined,
  test: async () => ({ success: true, message: 'ok' }),
  create: async () => undefined,
  update: async () => undefined,
}))

vi.mock('../modelStore', () => ({
  useModelStore: (selector: (state: typeof providerStoreState) => unknown) => selector(providerStoreState),
}))

import { ModelSettings } from './ModelSettings'

describe('ModelSettings colors', () => {
  it('shows enabled model configurations without a global active provider action', () => {
    const markup = renderToStaticMarkup(<ModelSettings />)

    expect(markup).toContain('deepseek-chat')
    expect(markup).toContain('text-status-danger-ink')
    expect(markup).not.toContain('使用中')
    expect(markup).not.toContain('设为当前服务商')
  })
})
