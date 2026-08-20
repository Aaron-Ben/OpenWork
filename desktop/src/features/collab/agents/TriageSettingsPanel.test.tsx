import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import { TriageSettingsContent } from './TriageSettingsPanel'

describe('TriageSettingsPanel', () => {
  it('makes the unconfigured fail-open state and provider/model controls visible', () => {
    const html = renderToStaticMarkup(
      <TriageSettingsContent
        settings={null}
        providers={[{
          id: 'provider', name: 'Provider', baseUrl: 'https://example.com', kind: 'openai', enabled: true,
          models: [{ modelId: 'model', displayName: 'Model', modelTier: 'lite', enabled: true }],
        }]}
        loading={false}
        saving={false}
        error={null}
        onSave={async () => true}
      />,
    )

    expect(html).toContain('data-triage-unconfigured="true"')
    expect(html).toContain('data-triage-provider')
    expect(html).toContain('data-triage-model')
    expect(html.match(/data-slot="select-trigger"/g)).toHaveLength(2)
    expect(html).toContain('Provider')
    expect(html).toContain('Model')
  })

  it('keeps transport diagnostics collapsed instead of flooding the settings page', () => {
    const diagnostic = 'invalid IPC request: unknown variant `triage_settings`, expected one of many internal variants'
    const html = renderToStaticMarkup(
      <TriageSettingsContent
        settings={null}
        providers={[]}
        loading={false}
        saving={false}
        error={diagnostic}
        onSave={async () => false}
      />,
    )

    expect(html).toContain('data-triage-layout="compact"')
    expect(html).toContain('data-triage-error-details="true"')
    expect(html).toMatch(/<details[^>]*data-triage-error-details="true"[\s\S]*invalid IPC request/)
  })
})
