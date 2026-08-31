import { existsSync, readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const forbidden = /features\/(chat|sessions|traces|models)(?:\/|')/
const importPattern = /from\s+['"]([^'"]+)['"]/g

function localImports(file: string): string[] {
  const source = readFileSync(file, 'utf8')
  return [...source.matchAll(importPattern)].flatMap((match) => {
    const specifier = match[1] ?? ''
    if (!specifier.startsWith('.') && !specifier.startsWith('@/')) return []
    const base = specifier.startsWith('@/')
      ? resolve(dirname(fileURLToPath(new URL('../App.tsx', import.meta.url))), specifier.slice(2))
      : resolve(dirname(file), specifier)
    for (const candidate of [`${base}.ts`, `${base}.tsx`, resolve(base, 'index.ts'), resolve(base, 'index.tsx')]) {
      if (existsSync(candidate)) return [candidate]
    }
    return []
  })
}

describe('CollabShell import boundary', () => {
  it('does not reach workbench feature modules through transitive imports', () => {
    const root = fileURLToPath(new URL('./CollabShell.tsx', import.meta.url))
    const pending = [root]
    const visited = new Set<string>()
    const violations: string[] = []
    while (pending.length > 0) {
      const file = pending.pop()
      if (!file || visited.has(file)) continue
      visited.add(file)
      const source = readFileSync(file, 'utf8')
      if (forbidden.test(source)) violations.push(file)
      pending.push(...localImports(file))
    }
    expect(violations).toEqual([])
  })
})
