import { useEffect, useRef } from 'react'
import { FilePenLine, X } from 'lucide-react'
import { motion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { FileDiffPanel, FileStats, type FileChangeView } from './FileDiffPanel'
import { workspaceDisplayName } from './FileChangePathLabel'

interface FileChangeReviewDrawerProps {
  changes: FileChangeView[]
  onClose: () => void
  workspaceRoot?: string
}

interface ReviewFileChange {
  change: FileChangeView
  sourceChangeIds: string[]
}

function groupChangesByFile(changes: FileChangeView[]): ReviewFileChange[] {
  const byPath = new Map<string, ReviewFileChange>()
  for (const change of changes) {
    const previous = byPath.get(change.path)
    byPath.set(change.path, previous ? {
      sourceChangeIds: [...previous.sourceChangeIds, change.changeId],
      change: {
        ...previous.change,
        kind: previous.change.kind === 'created' && change.kind === 'created'
          ? 'created'
          : 'modified',
        additions: previous.change.additions + change.additions,
        deletions: previous.change.deletions + change.deletions,
        hunks: [...previous.change.hunks, ...change.hunks],
        afterHash: change.afterHash,
        undone: previous.change.undone && change.undone,
      },
    } : {
      sourceChangeIds: [change.changeId],
      change,
    })
  }
  return [...byPath.values()]
}

/**
 * 审阅改动的右侧抽屉。
 *
 * 外壳(遮罩、贴边、进出动画、Esc、焦点归还)与 ContextWindowDrawer、TurnTraceDrawer
 * 逐项一致 —— 这三处都是"从对话里点开、看完就关"的详情视图，共用一套形态，用户不必
 * 为每个入口重新学一遍。宽度取 TurnTraceDrawer 的 1120：diff 和 trace 一样是宽内容，
 * 680 会把行号槽加代码挤到横向滚动。
 */
export function FileChangeReviewDrawer({
  changes,
  onClose,
  workspaceRoot,
}: FileChangeReviewDrawerProps) {
  const { t } = useTranslation()
  const drawerRef = useRef<HTMLElement>(null)
  const previousFocus = useRef<HTMLElement | null>(null)
  const additions = changes.reduce((total, change) => total + change.additions, 0)
  const deletions = changes.reduce((total, change) => total + change.deletions, 0)
  const files = groupChangesByFile(changes)
  const projectName = workspaceDisplayName(workspaceRoot)

  useEffect(() => {
    previousFocus.current = document.activeElement as HTMLElement | null
    drawerRef.current?.focus()
    return () => previousFocus.current?.focus()
  }, [])

  return (
    <motion.div
      className="fixed inset-0 z-40 bg-ink/10 backdrop-blur-[1px]"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onMouseDown={(event) => { if (event.target === event.currentTarget) onClose() }}
    >
      <motion.aside
        ref={drawerRef}
        role="dialog"
        aria-modal="true"
        aria-label={t('tool.reviewChanges')}
        data-file-change-review-drawer="true"
        tabIndex={-1}
        initial={{ opacity: 0, x: 32 }}
        animate={{ opacity: 1, x: 0 }}
        exit={{ opacity: 0, x: 32 }}
        className="absolute inset-y-0 right-0 flex w-[min(1120px,96vw)] flex-col border-l border-line bg-paper shadow-[-18px_0_45px_rgba(20,20,19,0.12)] outline-none max-[640px]:w-full"
        onKeyDown={(event) => { if (event.key === 'Escape') onClose() }}
      >
        <header className="flex items-center gap-3 border-b border-line px-5 py-3.5">
          <span className="grid size-9 shrink-0 place-items-center rounded-full bg-status-success-soft text-status-success-ink">
            <FilePenLine size={17} />
          </span>
          <div className="min-w-0 flex-1">
            <h2 className="truncate text-sm font-semibold text-ink">{t('tool.reviewChanges')}</h2>
            <div className="mt-0.5 flex min-w-0 items-center gap-2 text-xs">
              <span className="shrink-0 text-ink-faint">
                {t('tool.fileChangeCount', { count: files.length })}
              </span>
              <FileStats additions={additions} deletions={deletions} className="text-xs" />
              {projectName ? (
                <span className="truncate text-ink-faint">
                  {t('tool.inProject', { name: projectName })}
                </span>
              ) : null}
            </div>
          </div>
          <button
            type="button"
            data-file-change-review-close="true"
            aria-label={t('tool.closeReview')}
            title={t('tool.closeReview')}
            onClick={onClose}
            className="rounded-lg p-2 text-ink-soft transition-colors hover:bg-paper-hover hover:text-ink"
          >
            <X size={18} />
          </button>
        </header>

        <div className="min-h-0 flex-1 overflow-auto p-5 max-[640px]:p-3">
          <div
            data-file-change-review-list="true"
            className="overflow-hidden rounded-xl border border-line bg-paper shadow-sm"
          >
            {files.map(({ change, sourceChangeIds }, index) => (
              <div
                key={change.path}
                data-file-change-review-group={sourceChangeIds.join(' ')}
                className="border-b border-line last:border-b-0"
              >
                <FileDiffPanel
                  change={change}
                  compact
                  defaultExpanded={index === 0}
                  showCopyAction
                  workspaceRoot={workspaceRoot}
                />
              </div>
            ))}
          </div>
        </div>
      </motion.aside>
    </motion.div>
  )
}
