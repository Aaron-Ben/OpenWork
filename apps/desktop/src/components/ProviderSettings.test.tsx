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
  activeId: 'provider-1',
  presets: [],
  error: 'provider unavailable',
  activate: async () => undefined,
  remove: async () => undefined,
  test: async () => ({ success: true, message: 'ok' }),
  create: async () => undefined,
  update: async () => undefined,
}))

vi.mock('../stores/providerStore', () => ({
  useProviderStore: (selector: (state: typeof providerStoreState) => unknown) => selector(providerStoreState),
}))

import { ProviderSettings } from './ProviderSettings'

describe('ProviderSettings colors', () => {
  it('uses theme-aware status colors for the active provider and errors', () => {
    const markup = renderToStaticMarkup(<ProviderSettings />)

    expect(markup).toContain('bg-status-success-soft')
    expect(markup).toContain('text-status-success-ink')
    expect(markup).toContain('text-status-danger-ink')
    expect(markup).not.toContain('bg-emerald-50')
    expect(markup).not.toContain('text-rose-600')
  })
})
