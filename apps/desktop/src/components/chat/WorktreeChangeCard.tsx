import { useEffect, useState } from 'react'
import { ChevronDown, ChevronRight, FileCode2, RotateCcw } from 'lucide-react'

import { sessionsApi } from '../../api/sessions'
import { useSessionStore } from '../../stores/sessionStore'
import type {
  WorktreeSnapshotDetail,
  WorktreeSnapshotFileDetail,
  WorktreeSnapshotSummary,
} from '../../type/session'
import { DiffViewer } from './DiffViewer'

interface WorktreeChangeCardProps {
  sessionId: string
  snapshot: WorktreeSnapshotSummary
}

export function WorktreeChangeCard({ sessionId, snapshot }: WorktreeChangeCardProps) {
  const [expanded, setExpanded] = useState(false)
  const [detail, setDetail] = useState<WorktreeSnapshotDetail | null>(null)
  const [activePath, setActivePath] = useState<string | null>(null)
  const [isLoading, setIsLoading] = useState(false)
  const [isReverting, setIsReverting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const reloadSnapshots = useSessionStore((state) => state.reloadSnapshots)

  useEffect(() => {
    if (!expanded || detail || isLoading) return
    setIsLoading(true)
    setError(null)
    sessionsApi
      .worktreeSnapshotDetail(snapshot.id)
      .then((value) => {
        setDetail(value)
        setActivePath(value.files[0]?.path ?? null)
      })
      .catch((err: unknown) => setError(resolveErrorMessage(err)))
      .finally(() => setIsLoading(false))
  }, [detail, expanded, isLoading, snapshot.id])

  async function revert() {
    setIsReverting(true)
    setError(null)
    try {
      await sessionsApi.revertWorktreeSnapshot(snapshot.id)
      await reloadSnapshots(sessionId)
    } catch (err) {
      setError(resolveErrorMessage(err))
    } finally {
      setIsReverting(false)
    }
  }

  const activeFile = detail?.files.find((file) => file.path === activePath) ?? detail?.files[0]
  const reverted = snapshot.revertedAt !== null

  return (
    <section className="mx-auto mb-5 w-full max-w-[860px] overflow-hidden rounded-lg border border-line bg-paper shadow-sm">
      <div className="flex items-center justify-between gap-3 border-b border-line bg-paper-hover px-4 py-3">
        <button
          type="button"
          onClick={() => setExpanded((value) => !value)}
          className="flex min-w-0 flex-1 items-center gap-3 text-left"
          aria-expanded={expanded}
        >
          {expanded ? (
            <ChevronDown size={16} className="shrink-0 text-ink-faint" />
          ) : (
            <ChevronRight size={16} className="shrink-0 text-ink-faint" />
          )}
          <FileCode2 size={16} className="shrink-0 text-ink-faint" />
          <div className="min-w-0">
            <div className="text-sm font-semibold text-ink">
              Changed {snapshot.changedFiles.length} file
              {snapshot.changedFiles.length === 1 ? '' : 's'}
            </div>
            <div className="mt-0.5 truncate text-xs text-ink-faint">{snapshot.workingDir}</div>
          </div>
        </button>

        <button
          type="button"
          onClick={() => void revert()}
          disabled={isReverting || reverted}
          className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-md border border-line bg-paper px-3 text-xs font-medium text-ink-soft hover:bg-paper-hover disabled:cursor-not-allowed disabled:opacity-50"
        >
          <RotateCcw size={13} />
          {reverted ? 'Reverted' : isReverting ? 'Reverting' : 'Revert'}
        </button>
      </div>

      <div className="divide-y divide-line">
        {snapshot.changedFiles.slice(0, expanded ? undefined : 5).map((path) => (
          <div key={path} className="px-4 py-2 font-mono text-xs text-ink-soft">
            {path}
          </div>
        ))}
      </div>

      {expanded ? (
        <div className="border-t border-line p-3">
          {isLoading ? <div className="text-xs text-ink-faint">Loading diff</div> : null}
          {detail && activeFile ? (
            <div
              className={
                detail.files.length > 1
                  ? 'grid gap-3 md:grid-cols-[220px_minmax(0,1fr)]'
                  : 'grid gap-3'
              }
            >
              {detail.files.length > 1 ? (
                <div className="overflow-hidden rounded-md border border-line">
                  {detail.files.map((file) => (
                    <button
                      key={file.path}
                      type="button"
                      onClick={() => setActivePath(file.path)}
                      className={`block w-full truncate px-3 py-2 text-left font-mono text-xs ${
                        file.path === activeFile.path
                          ? 'bg-paper-hover text-ink'
                          : 'text-ink-soft hover:bg-paper-hover'
                      }`}
                      title={file.path}
                    >
                      {file.path}
                    </button>
                  ))}
                </div>
              ) : null}
              <FileDiff file={activeFile} />
            </div>
          ) : null}
          {error ? <div className="mt-3 text-xs text-rose-600">{error}</div> : null}
        </div>
      ) : null}
    </section>
  )
}

function FileDiff({ file }: { file: WorktreeSnapshotFileDetail }) {
  if (file.binary) {
    return (
      <div className="rounded-md border border-line bg-paper-hover px-3 py-2 text-xs text-ink-faint">
        Binary file changed.
      </div>
    )
  }
  return <DiffViewer filePath={file.path} oldString={file.beforeText ?? ''} newString={file.afterText ?? ''} />
}

function resolveErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return 'Unexpected error'
}
