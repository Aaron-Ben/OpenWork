import { useState } from 'react'
import { Check, Pencil, Trash2, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import type { RuntimeSessionRecord } from '@/bridge/compat'
import type { SessionRuntimePhase } from '@/features/chat/runtimeReducer'

type ItemMode = 'view' | 'edit' | 'confirm-delete'

/*
  会话行只有标题一行。
  时间与终态都拿掉了：侧栏是"找到那个会话"的地方，标题就是唯一的线索；
  当前会话与运行状态都由标题前的圆点表达，静止且未选中的会话不挂装饰。
*/
function dotToneClass(activity: SessionRuntimePhase): string {
  return activity === 'waiting_permission' ? 'bg-status-warning' : 'bg-clay'
}

interface SessionItemProps {
  session: RuntimeSessionRecord
  active: boolean
  activity: SessionRuntimePhase
  onSelect: () => void
  onRename: (title: string) => void
  onDelete: () => void
}

export function SessionItem({
  session,
  active,
  activity,
  onSelect,
  onRename,
  onDelete,
}: SessionItemProps) {
  const { t } = useTranslation()
  const [mode, setMode] = useState<ItemMode>('view')
  const [draft, setDraft] = useState(session.title ?? '')

  function commitRename() {
    const title = draft.trim()
    if (title && title !== session.title) onRename(title)
    else setDraft(session.title ?? '')
    setMode('view')
  }

  if (mode === 'edit') {
    return (
      <form
        className="flex items-center gap-1 rounded-lg bg-paper px-2 py-1.5"
        onSubmit={(event) => {
          event.preventDefault()
          commitRename()
        }}
      >
        <input
          autoFocus
          value={draft}
          aria-label={t('sidebar.sessionName')}
          className="min-w-0 flex-1 rounded-md border border-line bg-paper px-2 py-1 text-sm outline-none focus:border-clay"
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commitRename}
          onKeyDown={(event) => {
            if (event.key === 'Escape') {
              setDraft(session.title ?? '')
              setMode('view')
            }
          }}
        />
        <Button type="submit" variant="ghost" size="icon" className="size-6 text-status-success" aria-label={t('common.confirm')}>
          <Check size={13} />
        </Button>
      </form>
    )
  }

  if (mode === 'confirm-delete') {
    return (
      <div className="flex items-center gap-1 rounded-lg bg-status-danger-soft px-2 py-2">
        <span className="min-w-0 flex-1 truncate font-sans text-xs text-status-danger-ink">
          {t('sidebar.deleteSessionPrompt')}
        </span>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-6 text-status-danger-ink"
          aria-label={t('common.confirm')}
          onClick={() => {
            onDelete()
            setMode('view')
          }}
        >
          <Check size={13} />
        </Button>
        <Button type="button" variant="ghost" size="icon" className="size-6" aria-label={t('common.cancel')} onClick={() => setMode('view')}>
          <X size={13} />
        </Button>
      </div>
    )
  }

  return (
    <div className="group relative">
      <button
        type="button"
        data-session-row={session.id}
        onClick={onSelect}
        aria-current={active ? 'page' : undefined}
        className={`flex h-10 w-full items-center gap-2 rounded-full px-3 pr-14 text-left font-sans transition ${
          active ? 'bg-clay-soft' : 'hover:bg-paper'
        }`}
      >
        {active || activity !== 'idle' ? (
          <span
            className={`size-1.5 shrink-0 rounded-full ${dotToneClass(activity)}`}
            title={activity === 'waiting_permission' ? t('activity.needsInput') : t('sidebar.sessionRunning')}
          />
        ) : null}
        <span className={`min-w-0 flex-1 truncate text-sm ${active ? 'font-medium text-ink' : 'text-ink-soft group-hover:text-ink'}`}>
          {session.title?.trim() || t('sidebar.untitledSession')}
        </span>
      </button>
      <div className="absolute right-1.5 top-1/2 flex -translate-y-1/2 gap-0.5 opacity-0 transition group-hover:opacity-100 group-focus-within:opacity-100">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-6 bg-paper/80"
          aria-label={t('sidebar.renameSession')}
          onClick={() => {
            setDraft(session.title ?? '')
            setMode('edit')
          }}
        >
          <Pencil size={12} />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-6 bg-paper/80 hover:text-status-danger-ink"
          aria-label={t('sidebar.deleteSession')}
          onClick={() => setMode('confirm-delete')}
        >
          <Trash2 size={12} />
        </Button>
      </div>
    </div>
  )
}
