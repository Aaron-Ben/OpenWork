import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const stylesheet = readFileSync(new URL('./globals.css', import.meta.url), 'utf8')

function themeBlock(selector: ':root' | ':root.dark'): string {
  const match = stylesheet.match(
    new RegExp(`${selector.replace('.', '\\.') } \\{([\\s\\S]*?)\\n\\}`),
  )
  if (!match) throw new Error(`Missing ${selector} theme block`)
  return match[1]
}

function tokenValue(block: string, token: string): string {
  const match = block.match(new RegExp(`${token}:\\s*([^;]+);`))
  if (!match) throw new Error(`Missing ${token}`)
  return match[1].trim()
}

describe('Trace theme palette', () => {
  it('defines distinct light and dark values for every Waterfall semantic color', () => {
    const light = themeBlock(':root')
    const dark = themeBlock(':root.dark')
    const tokens = ['--trace-bar-model', '--trace-bar-tool']

    for (const token of tokens) {
      expect(tokenValue(light, token)).not.toBe(tokenValue(dark, token))
      expect(stylesheet).toContain(`--color-${token.slice(2)}: var(${token});`)
    }

  })

  it('defines theme-aware success, warning, and danger status surfaces', () => {
    const light = themeBlock(':root')
    const dark = themeBlock(':root.dark')
    const tokens = ['success', 'warning', 'danger'].flatMap((status) => [
      `--status-${status}`,
      `--status-${status}-soft`,
      `--status-${status}-ink`,
      `--status-${status}-border`,
    ])

    for (const token of tokens) {
      expect(tokenValue(light, token)).not.toBe(tokenValue(dark, token))
      expect(stylesheet).toContain(`--color-${token.slice(2)}: var(${token});`)
    }
  })
})
