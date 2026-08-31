import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

describe('root event bridge lifetime', () => {
  it('keeps the workbench bridge mounted without a legacy collaboration event bridge', () => {
    const app = readFileSync(new URL('../App.tsx', import.meta.url), 'utf8')
    const workbench = readFileSync(new URL('./AppShell.tsx', import.meta.url), 'utf8')
    expect(app).toContain('useCoreEventBridge()')
    expect(app).not.toContain('useCollabEventBridge')
    expect(app).toContain("mode === 'collab' ? <CollabShell />")
    expect(workbench).not.toContain('useCoreEventBridge')
  })
})
