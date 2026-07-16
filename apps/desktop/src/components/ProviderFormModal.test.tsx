import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import { providerTestResultStyle, ProviderFormModal } from './ProviderFormModal'

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
  })

  it('uses theme-aware semantic colors for connectivity results', () => {
    expect(providerTestResultStyle(true)).toContain('border-status-success-border')
    expect(providerTestResultStyle(true)).toContain('bg-status-success-soft')
    expect(providerTestResultStyle(true)).toContain('text-status-success-ink')
    expect(providerTestResultStyle(false)).toContain('border-status-danger-border')
    expect(providerTestResultStyle(false)).toContain('bg-status-danger-soft')
    expect(providerTestResultStyle(false)).toContain('text-status-danger-ink')
  })
})
