import { Copy, FileCode2 } from 'lucide-react'

type DiffViewerProps = {
  filePath: string
  oldString: string
  newString: string
}

type DiffLine =
  | { type: 'context'; text: string; oldLine: number | null; newLine: number | null }
  | { type: 'removed'; text: string; oldLine: number; newLine: null }
  | { type: 'added'; text: string; oldLine: null; newLine: number }

export function DiffViewer({ filePath, oldString, newString }: DiffViewerProps) {
  const lines = buildDiffLines(oldString, newString)
  const additions = lines.filter((line) => line.type === 'added').length
  const deletions = lines.filter((line) => line.type === 'removed').length
  const language = inferLanguage(filePath)

  async function copyPath() {
    await navigator.clipboard?.writeText(filePath)
  }

  return (
    <div className="overflow-hidden rounded-md border border-line bg-paper">
      <div className="flex items-center justify-between gap-3 border-b border-line bg-paper-hover px-3 py-2">
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-2">
            <FileCode2 size={14} className="shrink-0 text-ink-faint" />
            <div className="truncate font-mono text-xs text-ink-soft">{filePath}</div>
          </div>
          <div className="mt-1 flex items-center gap-2 text-[10px] uppercase tracking-wider">
            <span className="rounded bg-emerald-50 px-1.5 py-0.5 font-mono text-emerald-700">
              +{additions}
            </span>
            <span className="rounded bg-rose-50 px-1.5 py-0.5 font-mono text-rose-700">
              -{deletions}
            </span>
            <span className="text-ink-faint">{language}</span>
          </div>
        </div>
        <button
          type="button"
          onClick={() => void copyPath()}
          className="inline-flex h-7 shrink-0 items-center gap-1 rounded-md border border-line bg-paper px-2 text-[11px] text-ink-soft hover:bg-paper-hover"
        >
          <Copy size={12} />
          Copy path
        </button>
      </div>

      <div className="max-h-[420px] overflow-auto bg-ink">
        <table className="w-full border-collapse font-mono text-[11px] leading-relaxed text-paper">
          <tbody>
            {lines.map((line, index) => (
              <tr key={index} className={lineClass(line.type)}>
                <td className="w-10 select-none px-2 text-right text-paper/40">{line.oldLine ?? ''}</td>
                <td className="w-10 select-none border-r border-paper/10 px-2 text-right text-paper/40">
                  {line.newLine ?? ''}
                </td>
                <td className="w-5 select-none px-2 text-center text-paper/60">{lineSign(line.type)}</td>
                <td className="min-w-0 whitespace-pre-wrap break-words py-0.5 pr-3">
                  {line.text || ' '}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function lineClass(type: DiffLine['type']): string {
  if (type === 'added') return 'bg-emerald-500/15 text-emerald-50'
  if (type === 'removed') return 'bg-rose-500/15 text-rose-50'
  return 'text-paper/85'
}

function lineSign(type: DiffLine['type']): string {
  if (type === 'added') return '+'
  if (type === 'removed') return '-'
  return ''
}

function buildDiffLines(oldString: string, newString: string): DiffLine[] {
  const oldLines = oldString.split('\n')
  const newLines = newString.split('\n')
  const table = lcsTable(oldLines, newLines)
  const out: DiffLine[] = []
  let i = 0
  let j = 0
  let oldNo = 1
  let newNo = 1

  while (i < oldLines.length || j < newLines.length) {
    if (i < oldLines.length && j < newLines.length && oldLines[i] === newLines[j]) {
      out.push({ type: 'context', text: oldLines[i] ?? '', oldLine: oldNo, newLine: newNo })
      i += 1
      j += 1
      oldNo += 1
      newNo += 1
      continue
    }
    if (j < newLines.length && (i === oldLines.length || table[i][j + 1] >= table[i + 1][j])) {
      out.push({ type: 'added', text: newLines[j] ?? '', oldLine: null, newLine: newNo })
      j += 1
      newNo += 1
      continue
    }
    if (i < oldLines.length) {
      out.push({ type: 'removed', text: oldLines[i] ?? '', oldLine: oldNo, newLine: null })
      i += 1
      oldNo += 1
    }
  }
  return out
}

function lcsTable(left: string[], right: string[]): number[][] {
  const table = Array.from({ length: left.length + 1 }, () => Array(right.length + 1).fill(0))
  for (let i = left.length - 1; i >= 0; i -= 1) {
    for (let j = right.length - 1; j >= 0; j -= 1) {
      table[i][j] =
        left[i] === right[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1])
    }
  }
  return table
}

function inferLanguage(filePath: string): string {
  const ext = filePath.split('.').pop()?.toLowerCase()
  const langMap: Record<string, string> = {
    bash: 'bash',
    css: 'css',
    go: 'go',
    html: 'html',
    js: 'javascript',
    json: 'json',
    jsx: 'jsx',
    md: 'markdown',
    py: 'python',
    rb: 'ruby',
    rs: 'rust',
    sh: 'bash',
    sql: 'sql',
    toml: 'toml',
    ts: 'typescript',
    tsx: 'tsx',
    xml: 'xml',
    yaml: 'yaml',
    yml: 'yaml',
    zsh: 'bash',
  }
  return langMap[ext ?? ''] ?? 'text'
}
