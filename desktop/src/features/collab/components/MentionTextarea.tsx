import { useRef, useState, type Ref, type RefObject } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent } from '@/bridge/collab'
import { activeMention, insertMention, mentionCandidates } from '@/features/collab/rooms/mentionCompletion'
import { cn } from '@/lib/utils'

/** 什么按键提交：输入框是 Enter（Shift+Enter 换行），卡片描述是 Cmd/Ctrl+Enter。 */
export type SubmitKey = 'enter' | 'mod-enter'

/**
 * 带 `@` 补全的文本框（collaboration-desktop.md §7.4、§9）：候选依次为 `@all` 与 `agents` 中未归档的
 * Agent；补全打开时上下键选择、Enter/Tab 确认、Esc 关闭。
 */
export function MentionTextarea({
  value, onChange, agents, submitKey, onSubmit, onBlur, placeholder, rows, className, label, popupBelow = false, textareaRef,
}: {
  value: string
  onChange: (value: string) => void
  agents: CollabAgent[]
  submitKey: SubmitKey
  onSubmit: () => void
  onBlur?: () => void
  placeholder?: string
  rows?: number
  className?: string
  label: string
  popupBelow?: boolean
  textareaRef?: Ref<HTMLTextAreaElement>
}) {
  const { t } = useTranslation()
  const [caret, setCaret] = useState(0)
  const [dismissedAt, setDismissedAt] = useState<number | null>(null)
  const [choice, setChoice] = useState(0)
  const elementRef = useRef<HTMLTextAreaElement | null>(null)
  const mention = dismissedAt === caret ? null : activeMention(value, caret)
  const candidates = mention ? mentionCandidates(mention.query, agents) : []

  function track(element: HTMLTextAreaElement) {
    if (element.selectionStart !== caret) {
      setCaret(element.selectionStart)
      setChoice(0)
      setDismissedAt(null)
    }
  }

  function accept(element: HTMLTextAreaElement, id: string) {
    if (!mention) return
    const next = insertMention(value, mention, id)
    onChange(next.draft)
    setCaret(next.caret)
    globalThis.requestAnimationFrame(() => element.setSelectionRange(next.caret, next.caret))
  }

  function onKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (candidates.length > 0) {
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        const step = event.key === 'ArrowDown' ? 1 : -1
        setChoice((current) => (current + step + candidates.length) % candidates.length)
        return
      }
      if (event.key === 'Enter' || event.key === 'Tab') {
        event.preventDefault()
        const picked = candidates[choice] ?? candidates[0]
        if (picked) accept(event.currentTarget, picked.id)
        return
      }
      if (event.key === 'Escape') {
        event.preventDefault()
        setDismissedAt(caret)
        return
      }
    }
    if (event.key !== 'Enter' || event.nativeEvent.isComposing) return
    const modifier = event.metaKey || event.ctrlKey
    if ((submitKey === 'enter' && !event.shiftKey) || (submitKey === 'mod-enter' && modifier)) {
      event.preventDefault()
      onSubmit()
    }
  }

  return (
    <div className="relative">
      {candidates.length > 0 ? (
        <ul role="listbox" aria-label={label} className={cn('absolute left-0 z-20 max-h-60 w-72 overflow-y-auto rounded-xl border border-line bg-surface p-1 shadow-lg', popupBelow ? 'top-full mt-1' : 'bottom-full mb-2')}>
          {candidates.map((candidate, index) => (
            <li key={candidate.id} role="option" aria-selected={index === choice}>
              <button
                type="button"
                className={cn('flex w-full items-baseline gap-2 rounded-lg px-2.5 py-1.5 text-left text-sm', index === choice && 'bg-paper-hover')}
                onMouseDown={(event) => {
                  event.preventDefault()
                  if (elementRef.current) accept(elementRef.current, candidate.id)
                }}
              >
                <span className="font-semibold">@{candidate.id}</span>
                <span className="truncate text-xs text-ink-soft">
                  {'agent' in candidate
                    ? [candidate.agent.displayName, candidate.agent.role].filter(Boolean).join(' · ')
                    : t('collab.rooms.mentionAll')}
                </span>
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      <textarea
        ref={(node) => {
          elementRef.current = node
          assignRef(textareaRef, node)
        }}
        aria-label={label}
        rows={rows}
        value={value}
        placeholder={placeholder}
        className={cn('w-full resize-none bg-transparent text-ink outline-none placeholder:text-ink-faint', className)}
        onChange={(event) => { onChange(event.target.value); track(event.target) }}
        onSelect={(event) => track(event.currentTarget)}
        onKeyDown={onKeyDown}
        onBlur={onBlur}
      />
    </div>
  )
}

/** 把文本框同时交给调用方的 ref（函数或对象形式）。 */
function assignRef(ref: Ref<HTMLTextAreaElement> | undefined, node: HTMLTextAreaElement | null) {
  if (typeof ref === 'function') ref(node)
  else if (ref) (ref as RefObject<HTMLTextAreaElement | null>).current = node
}
