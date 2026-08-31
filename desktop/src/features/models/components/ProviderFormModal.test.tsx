import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import {
  buildProviderInput,
  providerTestResultStyle,
  ProviderFormModal,
  resolveProviderTestTarget,
} from './ProviderFormModal'

describe('ProviderFormModal layout', () => {
  it('uses a responsive two-column field grid without model hint copy', () => {
    const markup = renderToStaticMarkup(
      <ProviderFormModal open mode="create" onClose={vi.fn()} />,
    )

    expect(markup).toContain('data-provider-form-grid="true"')
    expect(markup).toContain('sm:grid-cols-2')
    expect(markup).toContain('max-w-3xl')
    expect(markup).not.toContain('快速或低成本模型')
    expect(markup).not.toContain('使用逗号分隔')
    expect(markup).not.toContain('协议')
    expect(markup).not.toContain('<select')
    expect(markup).toContain('sm:col-span-2')
    expect(markup).toContain('role="dialog"')
    expect(markup).toContain('aria-modal="true"')
  })

  it('uses theme-aware semantic colors for connectivity results', () => {
    expect(providerTestResultStyle(true)).toContain('border-status-success-border')
    expect(providerTestResultStyle(true)).toContain('bg-status-success-soft')
    expect(providerTestResultStyle(true)).toContain('text-status-success-ink')
    expect(providerTestResultStyle(false)).toContain('border-status-danger-border')
    expect(providerTestResultStyle(false)).toContain('bg-status-danger-soft')
    expect(providerTestResultStyle(false)).toContain('text-status-danger-ink')
  })

  it('allows an edit to keep the stored API key while create still requires one', () => {
    const draft = {
      name: 'DeepSeek',
      baseUrl: 'https://api.deepseek.com',
      apiKey: '',
      kind: 'deepseek' as const,
      liteModelsText: '',
      plusModelsText: 'deepseek-v4-flash',
      proModelsText: '',
      extraBodyText: '',
      capabilitiesByModel: {
        'deepseek-v4-flash': {
          contextWindowTokens: '200000',
          maxOutputTokens: '32768',
          maxReasoningTokens: '',
          acceptsDataBlocks: false,
        },
      },
    }

    expect(buildProviderInput(draft, 'edit')).toMatchObject({
      ok: true,
      input: { apiKey: '', models: [{ modelId: 'deepseek-v4-flash' }] },
    })
    expect(buildProviderInput(draft, 'create')).toEqual({
      ok: false,
      errorKey: 'apiKeyRequired',
    })
  })

  it('requires at least one enabled model before saving', () => {
    expect(buildProviderInput({
      name: 'DeepSeek',
      baseUrl: 'https://api.deepseek.com',
      apiKey: 'secret',
      kind: 'deepseek',
      liteModelsText: '',
      plusModelsText: '',
      proModelsText: '',
      extraBodyText: '',
      capabilitiesByModel: {},
    }, 'create')).toEqual({
      ok: false,
      errorKey: 'modelRequired',
    })
  })

  it('requires explicit model capabilities and applies the Core generation reserve rule', () => {
    const draft = {
      name: 'Custom',
      baseUrl: 'https://example.com',
      apiKey: 'secret',
      kind: 'openai' as const,
      liteModelsText: '',
      plusModelsText: 'custom-model',
      proModelsText: '',
      extraBodyText: '',
      capabilitiesByModel: {
        'custom-model': {
          contextWindowTokens: '',
          maxOutputTokens: '100',
          maxReasoningTokens: '20',
          acceptsDataBlocks: true,
        },
      },
    }

    expect(buildProviderInput(draft, 'create')).toEqual({
      ok: false,
      errorKey: 'contextWindowTokensInvalid',
      model: 'custom-model',
    })

    draft.capabilitiesByModel['custom-model'].contextWindowTokens = '120'
    expect(buildProviderInput(draft, 'create')).toEqual({
      ok: false,
      errorKey: 'generationReservationExhaustsWindow',
      model: 'custom-model',
    })

    draft.capabilitiesByModel['custom-model'].contextWindowTokens = '121'
    expect(buildProviderInput(draft, 'create')).toMatchObject({
      ok: true,
      input: {
        models: [{
          capabilities: {
            contextWindowTokens: 121,
            maxOutputTokens: 100,
            maxReasoningTokens: 20,
            acceptsDataBlocks: true,
          },
        }],
      },
    })
  })

  it('tests an edited model with the stored credential when no replacement key is entered', () => {
    expect(resolveProviderTestTarget('edit', 'provider-1', '')).toEqual({
      kind: 'stored',
      providerId: 'provider-1',
    })
    expect(resolveProviderTestTarget('edit', 'provider-1', 'replacement')).toEqual({
      kind: 'draft',
    })
    expect(resolveProviderTestTarget('create', undefined, '')).toEqual({ kind: 'draft' })
  })
})
