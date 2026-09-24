import { Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

/**
 * 列底部的“添加卡片”（collaboration-desktop.md §9，Cumora `BoardsView.tsx` 的 `ColumnView`）：
 * 点开后原地出现标题输入框，Enter 创建、Esc 取消、失焦时有内容就创建。
 */
export function AddCardInline({ onCreate }: { onCreate: (title: string) => Promise<void> }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [title, setTitle] = useState('')
  const [busy, setBusy] = useState(false)

  async function commit() {
    const trimmed = title.trim()
    if (!trimmed || busy) {
      setOpen(false)
      return
    }
    setBusy(true)
    try {
      await onCreate(trimmed)
      setTitle('')
      setOpen(false)
    } catch {
      // 失败原因由看板页的错误提示显示；保留输入，让用户改了再试。
    } finally {
      setBusy(false)
    }
  }

  if (!open) {
    return (
      <button type="button" className="flex items-center gap-1.5 rounded-lg px-2 py-1.5 text-xs text-ink-soft hover:bg-paper" onClick={() => setOpen(true)}>
        <Plus size={13} />{t('collab.boards.addCard')}
      </button>
    )
  }
  return (
    <input
      autoFocus
      value={title}
      disabled={busy}
      aria-label={t('collab.boards.addCard')}
      placeholder={t('collab.boards.cardTitlePlaceholder')}
      className="w-full rounded-xl border border-clay bg-paper px-3 py-2 text-[13px] outline-none"
      onChange={(event) => setTitle(event.target.value)}
      onBlur={() => void commit()}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && !event.nativeEvent.isComposing) {
          event.preventDefault()
          void commit()
        }
        if (event.key === 'Escape') {
          setTitle('')
          setOpen(false)
        }
      }}
    />
  )
}
