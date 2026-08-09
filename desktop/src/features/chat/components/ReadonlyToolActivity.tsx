import { useMemo, useState, type ReactNode } from 'react'
import {
  Activity,
  CircleAlert,
  CircleX,
  Copy,
  File,
  FileText,
  Files,
  Folder,
  ListTree,
  Loader2,
  Search,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import {
  isFailure,
  isInProgress,
  type ToolActivity,
} from '../toolActivity'
import {
  Separator,
  ToolActivityFrame,
} from './ToolActivityFrame'

export type ReadonlyToolName = 'read' | 'list' | 'glob' | 'grep'

interface ReadLine {
  number: number
  content: string
}

interface ListEntry {
  name: string
  directory: boolean
}

interface GrepMatch {
  path: string
  line: number
  content: string
}

interface ReadonlyActivityView {
  activity: ToolActivity
  tool: ReadonlyToolName
  action: string
  target: string
  range: string
  quantity: string
  readLines: ReadLine[]
  readHasMore: boolean
  listEntries: ListEntry[]
  globPaths: string[]
  grepMatches: GrepMatch[]
  grepFiles: string[]
  grepHitCount: number | null
  outputTruncated: boolean
}

interface ReadonlyToolActivityRowProps {
  activities: ToolActivity[]
  expanded?: boolean
  onExpandedChange: (expanded: boolean) => void
  onOpenTrace?: (providerToolCallId: string) => void
}

const DETAIL_LIMITS = {
  list: 50,
  glob: 30,
  grep: 50,
} as const

const READONLY_TOOL_ICONS: Record<ReadonlyToolName, typeof FileText> = {
  read: FileText,
  list: ListTree,
  glob: Files,
  grep: Search,
}

export function isReadonlyDisplayTool(name: string): name is ReadonlyToolName {
  return name === 'read' || name === 'list' || name === 'glob' || name === 'grep'
}

export function ReadonlyToolActivityRow({
  activities,
  expanded: rememberedExpanded,
  onExpandedChange,
  onOpenTrace,
}: ReadonlyToolActivityRowProps) {
  const { t } = useTranslation()
  const views = useMemo(
    () => activities.map((activity) => buildView(activity, (key, options) => t(key, options))),
    [activities, t],
  )
  const primary = views[0]
  if (!primary) return null
  const failed = activities.some((activity) => isFailure(activity))
  const statusActivity = activities.find((activity) => isInProgress(activity)) ?? primary.activity
  const defaultExpanded = failed || primary.tool === 'grep'
  const expanded = failed || (rememberedExpanded ?? defaultExpanded)
  const groupId = primary.activity.id
  const target = views.map((view) => view.target).filter(Boolean).join(', ')
  const range = sharedValue(views.map((view) => view.range))
  const quantity = failed ? '' : groupQuantity(views, (key, options) => t(key, options))

  const summary = (
    <>
      <span className="shrink-0 font-sans text-xs font-medium text-ink-soft" title={primary.tool}>
        {primary.action}
      </span>
      {target ? <Separator /> : null}
      {target ? (
        <span className="min-w-0 truncate font-mono text-xs text-ink" title={target}>
          {target}
        </span>
      ) : null}
      {range ? <Separator /> : null}
      {range ? <span className="shrink-0 text-xs text-ink-faint">{range}</span> : null}
      {views.length > 1 ? <Separator /> : null}
      {views.length > 1 ? (
        <span className="shrink-0 text-xs text-ink-faint">×{views.length}</span>
      ) : null}
      {quantity ? <Separator /> : null}
      {quantity ? <span className="shrink-0 text-xs text-ink-faint">{quantity}</span> : null}
    </>
  )

  async function copyFailure() {
    const error = activities.map((activity) => activity.output).filter(Boolean).join('\n\n')
    try {
      await navigator.clipboard?.writeText(error)
    } catch {
      // 剪贴板不可用时保持卡片原状；错误文本仍可手动选择。
    }
  }

  return (
    <ToolActivityFrame
      toolCallId={groupId}
      tier={failed ? 'failure' : 'readonly'}
      expanded={expanded}
      onExpandedChange={failed ? undefined : onExpandedChange}
      statusIcon={<ReadonlyStatusIcon activity={statusActivity} />}
      summary={summary}
      onOpenTrace={onOpenTrace}
      detailsMaxHeightClass="max-h-[240px]"
    >
      <div className="bg-paper/60">
        {views.map((view) => (
          <ReadonlyActivityDetails
            key={view.activity.id}
            view={view}
            grouped={views.length > 1}
            onOpenTrace={onOpenTrace}
          />
        ))}
        {failed ? (
          <div className="flex items-center gap-2 border-t border-status-danger-border px-3 py-2">
            <button
              type="button"
              onClick={() => void copyFailure()}
              className="inline-flex items-center gap-1.5 rounded-md border border-status-danger-border bg-paper px-2.5 py-1 text-xs text-status-danger-ink hover:bg-status-danger-soft"
            >
              <Copy size={11} />
              {t('tool.readonly.copyError')}
            </button>
            {onOpenTrace ? (
              <button
                type="button"
                onClick={() => onOpenTrace(primary.activity.id)}
                className="rounded-md border border-status-danger-border bg-paper px-2.5 py-1 text-xs text-status-danger-ink hover:bg-status-danger-soft"
              >
                {t('tool.readonly.inspectFailure')}
              </button>
            ) : null}
          </div>
        ) : null}
      </div>
    </ToolActivityFrame>
  )
}

function ReadonlyStatusIcon({
  activity,
}: {
  activity: ToolActivity
}) {
  const { t } = useTranslation()
  if (activity.state === 'pending' || activity.state === 'submitted' || activity.state === 'running') {
    return <Loader2 size={14} className="shrink-0 animate-spin text-ink-faint" aria-label={t('tool.running')} />
  }
  if (activity.state === 'error') {
    return <CircleAlert size={14} className="shrink-0 text-status-danger" aria-label={t('tool.error')} />
  }
  if (activity.state === 'denied' || activity.state === 'interrupted') {
    return <CircleX size={14} className="shrink-0 text-status-danger" aria-label={t('tool.stopped')} />
  }
  const tool = isReadonlyDisplayTool(activity.name) ? activity.name : 'read'
  const Icon = READONLY_TOOL_ICONS[tool]
  // 成功态图标是装饰：动作名就在紧邻的摘要里，读屏再念一遍只是噪音。
  return <Icon size={14} aria-hidden="true" className="shrink-0 text-ink-faint" />
}

function ReadonlyActivityDetails({
  view,
  grouped,
  onOpenTrace,
}: {
  view: ReadonlyActivityView
  grouped: boolean
  onOpenTrace?: (providerToolCallId: string) => void
}) {
  const { t } = useTranslation()
  const header = grouped ? (
    <div className="flex items-center gap-2 border-b border-line/60 bg-paper-hover/45 px-3 py-1.5 font-mono text-[11px] text-ink-soft">
      <span className="min-w-0 flex-1 truncate" title={view.target}>
        {view.target || view.activity.name}
      </span>
      {onOpenTrace ? (
        <button
          type="button"
          aria-label={t('activity.openToolSpan')}
          title={t('activity.openToolSpan')}
          onClick={() => onOpenTrace(view.activity.id)}
          className="grid size-5 shrink-0 place-items-center rounded text-ink-faint hover:bg-paper hover:text-clay"
        >
          <Activity size={11} />
        </button>
      ) : null}
    </div>
  ) : null

  return (
    <section
      className="[&+&]:border-t [&+&]:border-line"
      data-read-details={view.tool === 'read' ? view.activity.id : undefined}
      data-list-details={view.tool === 'list' ? view.activity.id : undefined}
      data-glob-details={view.tool === 'glob' ? view.activity.id : undefined}
      data-grep-details={view.tool === 'grep' ? view.activity.id : undefined}
    >
      {header}
      {isFailure(view.activity) ? (
        <pre className="whitespace-pre-wrap break-words px-3 py-2 font-mono text-xs leading-relaxed text-status-danger-ink">
          {view.activity.output || t('tool.readonly.noOutput')}
        </pre>
      ) : view.tool === 'read' ? (
        <ReadDetails view={view} />
      ) : view.tool === 'list' ? (
        <ListDetails view={view} />
      ) : view.tool === 'glob' ? (
        <GlobDetails view={view} />
      ) : (
        <GrepDetails view={view} />
      )}
    </section>
  )
}

function ReadDetails({ view }: { view: ReadonlyActivityView }) {
  const { t } = useTranslation()
  const [showAll, setShowAll] = useState(false)
  if (view.readLines.length === 0) return <RawOutput activity={view.activity} />
  const needsDomLimit = view.readLines.length > 2_000
  const visibleLines = needsDomLimit && !showAll
    ? [...view.readLines.slice(0, 200), ...view.readLines.slice(-200)]
    : view.readLines
  return (
    <div className="font-mono text-xs leading-5">
      {visibleLines.map((line, index) => (
        <div key={line.number}>
          {needsDomLimit && !showAll && index === 200 ? (
            <div className="border-y border-line/60 px-3 py-1.5 text-center text-ink-faint">
              {t('tool.readonly.hiddenLines', { count: view.readLines.length - 400 })}
            </div>
          ) : null}
          <div className="grid grid-cols-[3.5rem_minmax(0,1fr)]">
            <span className="select-none border-r border-line/70 px-2 text-right text-ink-faint">
              {line.number}
            </span>
            <span className="whitespace-pre px-3 text-ink-soft">{line.content || ' '}</span>
          </div>
        </div>
      ))}
      {needsDomLimit && !showAll ? (
        <div className="border-t border-line/60 px-3 py-2 text-center">
          <button
            type="button"
            onClick={() => setShowAll(true)}
            className="rounded-md border border-line bg-paper px-2.5 py-1 font-sans text-xs text-ink-soft hover:bg-paper-hover"
          >
            {t('tool.readonly.loadFullOutput')}
          </button>
        </div>
      ) : null}
      {view.readHasMore ? (
        <div className="border-t border-line/60 px-3 py-1.5 text-ink-faint">
          {t('tool.readonly.moreLinesAvailable')}
        </div>
      ) : null}
    </div>
  )
}

function ListDetails({ view }: { view: ReadonlyActivityView }) {
  const { t } = useTranslation()
  if (view.listEntries.length === 0) return <RawOutput activity={view.activity} />
  const sorted = [...view.listEntries].sort((left, right) =>
    Number(right.directory) - Number(left.directory),
  )
  const visible = sorted.slice(0, DETAIL_LIMITS.list)
  return (
    <div className="divide-y divide-line/50 px-2 py-1">
      {visible.map((entry) => {
        const Icon = entry.directory ? Folder : File
        return (
          <div key={`${entry.directory ? 'd' : 'f'}:${entry.name}`} className="flex items-center gap-2 px-1 py-1 font-mono text-xs text-ink-soft">
            <Icon size={13} className="shrink-0 text-ink-faint" />
            <span className="min-w-0 flex-1 truncate" title={entry.name}>{entry.name}</span>
          </div>
        )
      })}
      {sorted.length > visible.length ? (
        <div className="px-1 py-1.5 text-xs text-ink-faint">
          {t('tool.readonly.moreItems', { count: sorted.length - visible.length })}
        </div>
      ) : null}
    </div>
  )
}

function GlobDetails({ view }: { view: ReadonlyActivityView }) {
  const { t } = useTranslation()
  if (view.globPaths.length === 0) return <RawOutput activity={view.activity} />
  const visible = view.globPaths.slice(0, DETAIL_LIMITS.glob)
  const groups = groupPaths(visible)
  return (
    <div className="space-y-2 p-2">
      {groups.map(([directory, paths]) => (
        <div key={directory}>
          <div className="mb-1 px-1 font-mono text-[11px] text-ink-faint">{directory}</div>
          <div className="flex flex-wrap gap-1.5">
            {paths.map((path) => (
              <span key={path} title={path} className="max-w-full truncate rounded-md border border-line bg-paper px-2 py-0.5 font-mono text-[11px] text-ink-soft">
                {baseName(path)}
              </span>
            ))}
          </div>
        </div>
      ))}
      {view.globPaths.length > visible.length ? (
        <div className="px-1 text-xs text-ink-faint">
          {t('tool.readonly.moreItems', { count: view.globPaths.length - visible.length })}
        </div>
      ) : null}
    </div>
  )
}

function GrepDetails({ view }: { view: ReadonlyActivityView }) {
  const { t } = useTranslation()
  if (view.grepMatches.length === 0) {
    if (view.grepFiles.length > 0) {
      return (
        <div className="flex flex-wrap gap-1.5 p-2">
          {view.grepFiles.slice(0, DETAIL_LIMITS.grep).map((path) => (
            <span key={path} className="rounded-md border border-line bg-paper px-2 py-0.5 font-mono text-[11px] text-ink-soft">
              {path}
            </span>
          ))}
        </div>
      )
    }
    return <RawOutput activity={view.activity} />
  }

  const allGroups = groupGrepMatches(view.grepMatches)
  const grouped = limitGrepGroups(allGroups, DETAIL_LIMITS.grep)
  const visibleCount = grouped.reduce((total, [, matches]) => total + matches.length, 0)
  const totalByFile = new Map(allGroups.map(([path, matches]) => [path, matches.length]))
  return (
    <div className="divide-y divide-line/70">
      {grouped.map(([path, matches]) => (
        <section key={path}>
          <div className="sticky top-0 z-[1] border-b border-line/50 bg-paper-hover px-3 py-1.5 font-mono text-[11px] text-ink-soft">
            {path} · {t('tool.readonly.hitCount', { count: totalByFile.get(path) ?? matches.length })}
          </div>
          <div className="font-mono text-xs leading-5">
            {matches.map((match, index) => (
              <div key={`${match.line}:${index}`} className="grid grid-cols-[3.5rem_minmax(0,1fr)]">
                <span className="select-none border-r border-line/70 px-2 text-right text-ink-faint">
                  {match.line}
                </span>
                <span className="whitespace-pre px-3 text-ink-soft">
                  {highlightMatches(match.content, stringInput(view.activity.input, 'pattern'))}
                </span>
              </div>
            ))}
          </div>
        </section>
      ))}
      {view.grepMatches.length > visibleCount ? (
        <div className="px-3 py-2 text-xs text-ink-faint">
          {t('tool.readonly.moreHits', { count: view.grepMatches.length - visibleCount })}
        </div>
      ) : null}
    </div>
  )
}

function RawOutput({ activity }: { activity: ToolActivity }) {
  const { t } = useTranslation()
  return (
    <pre className="whitespace-pre-wrap break-words px-3 py-2 font-mono text-xs leading-relaxed text-ink-soft">
      {activity.output || t('tool.readonly.noOutput')}
    </pre>
  )
}

function buildView(
  activity: ToolActivity,
  translate: (key: string, options?: Record<string, unknown>) => string,
): ReadonlyActivityView {
  const tool = activity.name as ReadonlyToolName
  const target = tool === 'glob' || tool === 'grep'
    ? stringInput(activity.input, 'pattern')
    : stringInput(activity.input, 'path') || stringInput(activity.input, 'filePath')
  const read = parseReadOutput(activity.output)
  const listEntries = tool === 'list' ? parseListOutput(activity.output) : []
  const glob = tool === 'glob' ? parsePathOutput(activity.output) : {
    paths: [],
    truncated: false,
  }
  const grep = tool === 'grep' ? parseGrepOutput(activity) : {
    matches: [],
    files: [],
    hitCount: null,
    truncated: false,
  }
  const grepScope = tool === 'grep' ? grepScopeFromInput(activity.input) : ''
  let range = ''
  let quantity = ''

  if (tool === 'read') {
    if (read.lines.length > 0) {
      const first = read.lines[0].number
      const last = read.lines[read.lines.length - 1].number
      const offset = numberInput(activity.input, 'offset') ?? 0
      const limit = numberInput(activity.input, 'limit')
      const wholeFile = offset === 0 && limit == null && first === 1 && !read.hasMore
      range = wholeFile
        ? translate('tool.readonly.wholeFile')
        : translate('tool.readonly.lineRange', { start: first, end: last })
      quantity = wholeFile
        ? translate('tool.readonly.lineCount', { count: read.lines.length })
        : read.total == null
          ? translate('tool.readonly.lineCount', { count: read.lines.length })
          : translate('tool.readonly.totalLineCount', { count: read.total })
    } else {
      const offset = numberInput(activity.input, 'offset') ?? 0
      const limit = numberInput(activity.input, 'limit')
      const wholeFile = offset === 0 && limit == null
      range = wholeFile
        ? translate('tool.readonly.wholeFile')
        : limit == null
          ? translate('tool.readonly.fromLine', { line: offset + 1 })
          : translate('tool.readonly.lineRange', { start: offset + 1, end: offset + limit })
      if (activity.state === 'success') {
        quantity = wholeFile
          ? translate('tool.readonly.lineCount', { count: 0 })
          : read.total == null
            ? translate('tool.readonly.lineCount', { count: 0 })
            : translate('tool.readonly.totalLineCount', { count: read.total })
      }
    }
  } else if (tool === 'list') {
    const directories = listEntries.filter((entry) => entry.directory).length
    const files = listEntries.length - directories
    quantity = activity.output || activity.state === 'success'
      ? listQuantity(directories, files, translate)
      : ''
  } else if (tool === 'glob') {
    const path = stringInput(activity.input, 'path')
    range = path && path !== '.' ? translate('tool.readonly.inScope', { scope: path }) : ''
    quantity = activity.output || activity.state === 'success'
      ? translate(
          glob.truncated ? 'tool.readonly.atLeastFileCount' : 'tool.readonly.fileCount',
          { count: glob.paths.length },
        )
      : ''
  } else {
    range = grepScope ? translate('tool.readonly.inScope', { scope: grepScope }) : ''
    if ((activity.output || activity.state === 'success') && grep.hitCount != null) {
      quantity = translate(grep.truncated
        ? 'tool.readonly.atLeastHitsAndFiles'
        : 'tool.readonly.hitsAndFiles', {
        hits: grep.hitCount,
        files: grep.files.length,
      })
    } else if (activity.output || activity.state === 'success') {
      quantity = translate(
        grep.truncated ? 'tool.readonly.atLeastFileCount' : 'tool.readonly.fileCount',
        { count: grep.files.length },
      )
    }
  }

  return {
    activity,
    tool,
    action: translate(`tool.readonly.${tool}Action`),
    target,
    range,
    quantity,
    readLines: read.lines,
    readHasMore: read.hasMore,
    listEntries,
    globPaths: glob.paths,
    grepMatches: grep.matches,
    grepFiles: grep.files,
    grepHitCount: grep.hitCount,
    outputTruncated: tool === 'glob' ? glob.truncated : tool === 'grep' ? grep.truncated : false,
  }
}

function groupQuantity(
  views: ReadonlyActivityView[],
  translate: (key: string, options?: Record<string, unknown>) => string,
): string {
  if (views.length === 1) return views[0].quantity
  switch (views[0].tool) {
    case 'read':
      if (views.every((view) => !view.quantity)) return ''
      return translate('tool.readonly.lineCount', {
        count: views.reduce((total, view) => total + view.readLines.length, 0),
      })
    case 'list': {
      if (views.every((view) => !view.quantity)) return ''
      const entries = views.flatMap((view) => view.listEntries)
      const directories = entries.filter((entry) => entry.directory).length
      return listQuantity(directories, entries.length - directories, translate)
    }
    case 'glob':
      if (views.every((view) => !view.quantity)) return ''
      return translate(
        views.some((view) => view.outputTruncated)
          ? 'tool.readonly.atLeastFileCount'
          : 'tool.readonly.fileCount', {
        count: views.reduce((total, view) => total + view.globPaths.length, 0),
        },
      )
    case 'grep': {
      const files = new Set(views.flatMap((view) => view.grepFiles)).size
      if (views.every((view) => !view.quantity)) return ''
      const hasUnknownHits = views.some((view) => view.quantity && view.grepHitCount == null)
      const truncated = views.some((view) => view.outputTruncated)
      if (hasUnknownHits) {
        return translate(
          truncated ? 'tool.readonly.atLeastFileCount' : 'tool.readonly.fileCount',
          { count: files },
        )
      }
      const hits = views.reduce((total, view) => total + (view.grepHitCount ?? 0), 0)
      return translate(
        truncated ? 'tool.readonly.atLeastHitsAndFiles' : 'tool.readonly.hitsAndFiles',
        { hits, files },
      )
    }
  }
}

function listQuantity(
  directories: number,
  files: number,
  translate: (key: string, options?: Record<string, unknown>) => string,
): string {
  return [
    translate('tool.readonly.directoryCount', { count: directories }),
    translate('tool.readonly.fileCount', { count: files }),
  ].join(' · ')
}

function parseReadOutput(output: string): { lines: ReadLine[]; hasMore: boolean; total: number | null } {
  const lines: ReadLine[] = []
  let hasMore = false
  let total: number | null = null
  for (const line of output.split('\n')) {
    const match = line.match(/^\s*(\d+)\t(.*)$/)
    if (match) {
      lines.push({ number: Number(match[1]), content: match[2] })
    } else if (/^\[more lines available at offset \d+\]$/.test(line)) {
      hasMore = true
    } else {
      const emptyPage = line.match(/^no lines at offset \d+ \(total (\d+)\)$/)
      if (emptyPage) total = Number(emptyPage[1])
    }
  }
  if (!hasMore && lines.length > 0) total = lines[lines.length - 1].number
  return { lines, hasMore, total }
}

function parseListOutput(output: string): ListEntry[] {
  const lines = output
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line && !/^\[showing \d+ entries from offset \d+; more entries available at offset \d+\]$/.test(line))
  if (lines.length === 1 && (/ is empty$/.test(lines[0]) || /^no entries at offset /.test(lines[0]))) {
    return []
  }
  return lines
    .map((name) => ({ name, directory: name.endsWith('/') }))
}

function parsePathOutput(output: string): { paths: string[]; truncated: boolean } {
  const parsed = completeOutputLines(output)
  return {
    paths: parsed.lines
      .map((line) => line.trim())
      .filter((line) => line && !/^\[result limit reached at \d+; more matches may exist\]$/.test(line) && line !== 'no files matched'),
    truncated: parsed.truncated,
  }
}

function parseGrepOutput(activity: ToolActivity): {
  matches: GrepMatch[]
  files: string[]
  hitCount: number | null
  truncated: boolean
} {
  const mode = stringInput(activity.input, 'outputMode')
    || stringInput(activity.input, 'output_mode')
    || 'content'
  const parsed = completeOutputLines(activity.output)
  const lines = parsed.lines
    .map((line) => line.trimEnd())
    .filter((line) => line && !/^\[result limit reached at \d+; more matches may exist\]$/.test(line) && !line.startsWith('no matches for /'))

  if (mode === 'files_with_matches') {
    return { matches: [], files: lines, hitCount: null, truncated: parsed.truncated }
  }
  if (mode === 'count') {
    let hits = 0
    const files: string[] = []
    for (const line of lines) {
      const match = line.match(/^(.*):(\d+)$/)
      if (!match) continue
      files.push(match[1])
      hits += Number(match[2])
    }
    return { matches: [], files, hitCount: hits, truncated: parsed.truncated }
  }

  const matches = lines.flatMap((line) => {
    const match = line.match(/^(.*?):(\d+):(.*)$/)
    return match
      ? [{ path: match[1], line: Number(match[2]), content: match[3] }]
      : []
  })
  return {
    matches,
    files: [...new Set(matches.map((match) => match.path))],
    hitCount: matches.length,
    truncated: parsed.truncated,
  }
}

function grepScopeFromInput(input: Record<string, unknown> | null): string {
  const path = stringInput(input, 'path') || '.'
  const glob = stringInput(input, 'glob')
  if (!glob) return path
  if (path === '.') return glob
  return `${trimSlash(path)}/${glob}`
}

function completeOutputLines(output: string): { lines: string[]; truncated: boolean } {
  const lines = output.split('\n')
  const resultLimited = lines.some((line) =>
    /^\[result limit reached at \d+; more matches may exist\]$/.test(line.trim()),
  )
  const markerIndex = lines.findIndex((line) => /^\.\.\.\[\d+ 行已隐藏\]\.\.\.$/.test(line.trim()))
  if (markerIndex < 0) return { lines, truncated: resultLimited }

  // truncate_output 按字节切割，标记两侧的相邻行都可能只是半行，不能当完整路径或命中。
  return {
    lines: lines.filter((_, index) =>
      index < markerIndex - 1 || index > markerIndex + 1,
    ),
    truncated: true,
  }
}

function groupPaths(paths: string[]): Array<[string, string[]]> {
  const grouped = new Map<string, string[]>()
  for (const path of paths) {
    const separator = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
    const directory = separator >= 0 ? path.slice(0, separator) : '.'
    const existing = grouped.get(directory) ?? []
    existing.push(path)
    grouped.set(directory, existing)
  }
  return [...grouped]
}

function groupGrepMatches(matches: GrepMatch[]): Array<[string, GrepMatch[]]> {
  const grouped = new Map<string, GrepMatch[]>()
  for (const match of matches) {
    const existing = grouped.get(match.path) ?? []
    existing.push(match)
    grouped.set(match.path, existing)
  }
  return [...grouped]
}

function limitGrepGroups(
  groups: Array<[string, GrepMatch[]]>,
  limit: number,
): Array<[string, GrepMatch[]]> {
  const visible = groups.map(([path]) => [path, [] as GrepMatch[]] as [string, GrepMatch[]])
  let added = 0
  let offset = 0
  let progressed = true
  while (added < limit && progressed) {
    progressed = false
    for (let index = 0; index < groups.length && added < limit; index += 1) {
      const match = groups[index][1][offset]
      if (!match) continue
      visible[index][1].push(match)
      added += 1
      progressed = true
    }
    offset += 1
  }
  return visible.filter(([, matches]) => matches.length > 0)
}

function highlightMatches(content: string, pattern: string): ReactNode {
  const parsed = safeLiteralPattern(pattern)
  if (!parsed || !parsed.literal) return content
  const haystack = parsed.caseInsensitive ? asciiLower(content) : content
  const needle = parsed.caseInsensitive ? asciiLower(parsed.literal) : parsed.literal
  const fragments: ReactNode[] = []
  let cursor = 0
  while (cursor < content.length) {
    const index = haystack.indexOf(needle, cursor)
    if (index < 0) break
    if (index > cursor) fragments.push(content.slice(cursor, index))
    fragments.push(
      <mark key={index} className="rounded-sm bg-clay-soft px-0.5 text-ink">
        {content.slice(index, index + parsed.literal.length)}
      </mark>,
    )
    cursor = index + parsed.literal.length
  }
  if (fragments.length === 0) return content
  if (cursor < content.length) fragments.push(content.slice(cursor))
  return fragments
}

function asciiLower(value: string): string {
  return value.replace(/[A-Z]/g, (character) => character.toLowerCase())
}

function safeLiteralPattern(pattern: string): { literal: string; caseInsensitive: boolean } | null {
  if (!pattern || pattern.length > 512) return null
  let source = pattern
  let caseInsensitive = false
  if (source.startsWith('(?i)')) {
    source = source.slice(4)
    caseInsensitive = true
  }

  let literal = ''
  const metacharacters = new Set(['.', '*', '+', '?', '^', '$', '{', '}', '(', ')', '|', '[', ']'])
  for (let index = 0; index < source.length; index += 1) {
    const character = source[index]
    if (character === '\\') {
      const escaped = source[index + 1]
      if (!escaped || !metacharacters.has(escaped) && escaped !== '\\') return null
      literal += escaped
      index += 1
    } else {
      if (metacharacters.has(character)) return null
      literal += character
    }
  }
  return { literal, caseInsensitive }
}

function sharedValue(values: string[]): string {
  if (values.length === 0 || !values[0]) return ''
  return values.every((value) => value === values[0]) ? values[0] : ''
}

function stringInput(input: Record<string, unknown> | null, key: string): string {
  const value = input?.[key]
  return typeof value === 'string' ? value : ''
}

function numberInput(input: Record<string, unknown> | null, key: string): number | null {
  const value = input?.[key]
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function trimSlash(value: string): string {
  return value.replace(/[\\/]+$/, '')
}

function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path
}
