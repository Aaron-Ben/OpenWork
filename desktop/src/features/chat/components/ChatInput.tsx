import { useEffect, useMemo, useRef, useState, type ReactNode, type Ref } from 'react'
import { ArrowUp, LoaderCircle, Minimize2, Puzzle, ShieldCheck, Square } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { ProviderModel } from '@/features/models/contracts'
import type {
  RuntimePermissionMode,
  RuntimeSkillInput,
  RuntimeSkillSummary,
} from '@/bridge/compat'
import type { ContextUsage, ContextUsageBreakdown } from '../contextUsage'
import {
  findSkillMentionTarget,
  deriveSkillMentionEdit,
  rankSkillCandidates,
  reconcileSkillMentionBindings,
  selectSkillMention,
  skillMentionSegments,
  skillInputsFromBindings,
  validSkillMentionBindings,
  type SkillMentionBinding,
} from '../skillMentions'
import { ContextUsageIndicator } from './ContextUsageIndicator'

interface ChatInputProps {
  model: string
  modelOptions: ProviderModel[]
  modelSelectionLocked?: boolean
  permissionMode: RuntimePermissionMode
  value: string
  isSending: boolean
  isCompacting?: boolean
  disabled?: boolean
  skills?: readonly RuntimeSkillSummary[]
  onValueChange: (value: string) => void
  onModelChange: (model: string) => void
  onPermissionModeChange: (mode: RuntimePermissionMode) => void
  onSubmit: (skills: RuntimeSkillInput[]) => void
  onCancel?: () => void
  topContent?: ReactNode
  contextUsage?: ContextUsage | null
  contextBreakdown?: ContextUsageBreakdown | null
  contextInspectorOpen?: boolean
  onInspectContext?: () => void
  onRefreshSkills?: () => void | Promise<void>
  onSlashCommand?: (command: ChatSlashCommand) => void
}

export type ChatSlashCommand = 'compact'

interface SkillMentionOverlayProps {
  value: string
  bindings: readonly SkillMentionBinding[]
  overlayRef?: Ref<HTMLDivElement>
  contentRef?: Ref<HTMLDivElement>
}

export function SkillMentionOverlay({
  value,
  bindings,
  overlayRef,
  contentRef,
}: SkillMentionOverlayProps) {
  const segments = skillMentionSegments(value, bindings)
  return (
    <div
      ref={overlayRef}
      data-skill-mention-overlay="true"
      aria-hidden="true"
      /*
        排版必须与下面的 textarea 逐项一致：字体、字号、行高、内边距。
        用户看到的字是这一层，光标和选区却由 textarea 按它自己的字体度量绘制 ——
        任何一项对不上，光标就会随着文字变长越偏越远，选中时还会露出错位的重影。
        改这里的时候，textarea 的 className 要同步改。
      */
      className="pointer-events-none absolute inset-0 overflow-hidden px-5 py-3 font-serif text-base leading-7 text-ink whitespace-pre-wrap [overflow-wrap:break-word]"
    >
      <div ref={contentRef}>
        {segments.map((segment, index) => segment.binding ? (
          <span
            key={`${segment.binding.start}:${segment.binding.path}`}
            data-skill-mention={segment.binding.name}
            className="rounded-[3px] bg-clay-soft text-clay shadow-[inset_0_0_0_1px_color-mix(in_srgb,currentColor_18%,transparent)]"
          >
            {segment.text}
          </span>
        ) : <span key={index}>{segment.text}</span>)}
        {value.endsWith('\n') ? '\u200b' : null}
      </div>
    </div>
  )
}

function resizeTextarea(textarea: HTMLTextAreaElement) {
  textarea.style.height = 'auto'
  textarea.style.height = `${Math.min(textarea.scrollHeight, 200)}px`
}

export function ChatInput({
  model,
  modelOptions,
  modelSelectionLocked = false,
  permissionMode,
  value,
  isSending,
  isCompacting = false,
  disabled = false,
  skills = [],
  onValueChange,
  onModelChange,
  onPermissionModeChange,
  onSubmit,
  onCancel,
  topContent,
  contextUsage,
  contextBreakdown,
  contextInspectorOpen = false,
  onInspectContext,
  onRefreshSkills,
  onSlashCommand,
}: ChatInputProps) {
  const { t } = useTranslation()
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const overlayRef = useRef<HTMLDivElement>(null)
  const overlayContentRef = useRef<HTMLDivElement>(null)
  const skillOptionRefs = useRef<Array<HTMLButtonElement | null>>([])
  const previousValueRef = useRef(value)
  const selectionRef = useRef({ start: value.length, end: value.length })
  const compositionSelectionRef = useRef<{ start: number; end: number } | null>(null)
  const composingRef = useRef(false)
  const [slashMenuDismissed, setSlashMenuDismissed] = useState(false)
  const [skillMenuDismissed, setSkillMenuDismissed] = useState(false)
  const [caret, setCaret] = useState(value.length)
  const [selectedSkillIndex, setSelectedSkillIndex] = useState(0)
  const [skillBindings, setSkillBindings] = useState<SkillMentionBinding[]>([])
  const selectedModel = modelOptions.find((option) => option.modelId === model)
  const skillTarget = useMemo(() => {
    const target = findSkillMentionTarget(value, caret)
    if (!target) return null
    const overlapsBinding = validSkillMentionBindings(value, skillBindings).some(
      (binding) => target.start < binding.end && target.end > binding.start,
    )
    return overlapsBinding ? null : target
  }, [caret, skillBindings, value])
  const skillCandidates = useMemo(
    () => skillTarget ? rankSkillCandidates(skills, skillTarget.query) : [],
    [skillTarget, skills],
  )
  const activeSkillIndex = Math.min(selectedSkillIndex, Math.max(0, skillCandidates.length - 1))
  const slashQuery = value.startsWith('/') && !/\s/.test(value)
    ? value.slice(1).toLowerCase()
    : null
  const compactMatches = slashQuery !== null && 'compact'.startsWith(slashQuery)
  const slashMenuOpen = compactMatches
    && skillTarget === null
    && !slashMenuDismissed
    && !disabled
    && !isSending
    && !isCompacting
  const skillMenuOpen = skillTarget !== null
    && skillCandidates.length > 0
    && !skillMenuDismissed
    && !disabled
    && !isSending
    && !isCompacting

  useEffect(() => {
    const textarea = textareaRef.current
    if (!textarea || composingRef.current) return
    const frame = window.requestAnimationFrame(() => {
      resizeTextarea(textarea)
      syncOverlay(textarea)
    })
    return () => window.cancelAnimationFrame(frame)
  }, [value])

  useEffect(() => setSlashMenuDismissed(false), [value])
  useEffect(() => setSkillMenuDismissed(false), [value, caret])
  useEffect(
    () => setSelectedSkillIndex(0),
    [skillCandidates.length, skillTarget?.query, skillTarget?.start],
  )

  useEffect(() => {
    if (!skillMenuOpen) return
    skillOptionRefs.current[activeSkillIndex]?.scrollIntoView({ block: 'nearest' })
  }, [activeSkillIndex, skillMenuOpen])

  useEffect(() => {
    if (skillTarget?.query === '') void onRefreshSkills?.()
  }, [onRefreshSkills, skillTarget?.query])

  useEffect(() => {
    const previousValue = previousValueRef.current
    if (previousValue === value) return
    setSkillBindings([])
    previousValueRef.current = value
    compositionSelectionRef.current = null
    setCaret((current) => {
      const position = Math.min(current, value.length)
      selectionRef.current = { start: position, end: position }
      return position
    })
  }, [value])

  function syncSelection(textarea: HTMLTextAreaElement) {
    const start = textarea.selectionStart ?? value.length
    const end = textarea.selectionEnd ?? start
    selectionRef.current = { start, end }
    setCaret(start)
  }

  function syncOverlay(textarea: HTMLTextAreaElement) {
    if (overlayRef.current) {
      const scrollbarWidth = Math.max(0, textarea.offsetWidth - textarea.clientWidth)
      overlayRef.current.style.right = `${scrollbarWidth}px`
    }
    if (!overlayContentRef.current) return
    overlayContentRef.current.style.transform = `translate(${-textarea.scrollLeft}px, ${-textarea.scrollTop}px)`
  }

  function handleValueChange(
    nextValue: string,
    selectionStart: number | null,
    selectionEnd: number | null,
  ) {
    const previousSelection = compositionSelectionRef.current ?? selectionRef.current
    const edit = deriveSkillMentionEdit(value, nextValue, previousSelection, selectionStart)
    setSkillBindings((current) => reconcileSkillMentionBindings(value, nextValue, current, edit))
    previousValueRef.current = nextValue
    const nextStart = selectionStart ?? nextValue.length
    selectionRef.current = { start: nextStart, end: selectionEnd ?? nextStart }
    if (compositionSelectionRef.current) {
      compositionSelectionRef.current = {
        start: previousSelection?.start ?? nextStart,
        end: nextStart,
      }
    }
    setCaret(nextStart)
    onValueChange(nextValue)
  }

  function selectCompactCommand() {
    if (onSlashCommand) {
      onSlashCommand('compact')
    } else {
      onValueChange('/compact')
    }
  }

  function selectSkillCandidate(skill: RuntimeSkillSummary) {
    if (!skillTarget) return
    const selected = selectSkillMention(value, skillTarget, skill)
    setSkillBindings((current) => [
      ...reconcileSkillMentionBindings(value, selected.value, current, skillTarget),
      selected.binding,
    ])
    previousValueRef.current = selected.value
    selectionRef.current = { start: selected.caret, end: selected.caret }
    setCaret(selected.caret)
    setSkillMenuDismissed(true)
    onValueChange(selected.value)
    window.requestAnimationFrame(() => {
      textareaRef.current?.focus()
      textareaRef.current?.setSelectionRange(selected.caret, selected.caret)
    })
  }

  function submit() {
    onSubmit(skillInputsFromBindings(value, skillBindings))
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    syncSelection(event.currentTarget)
    if (composingRef.current || event.nativeEvent.isComposing || event.keyCode === 229) return
    if (skillMenuOpen) {
      if (event.key === 'Escape') {
        event.preventDefault()
        setSkillMenuDismissed(true)
        return
      }
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        const direction = event.key === 'ArrowDown' ? 1 : -1
        setSelectedSkillIndex((current) => (
          (current + direction + skillCandidates.length) % skillCandidates.length
        ))
        return
      }
      if (event.key === 'Enter' || event.key === 'Tab') {
        event.preventDefault()
        const selected = skillCandidates[activeSkillIndex]
        if (selected) selectSkillCandidate(selected)
        return
      }
    }
    if (slashMenuOpen) {
      if (event.key === 'Escape') {
        event.preventDefault()
        setSlashMenuDismissed(true)
        return
      }
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        return
      }
      if (event.key === 'Enter' || event.key === 'Tab') {
        event.preventDefault()
        selectCompactCommand()
        return
      }
    }
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault()
      submit()
    }
  }

  return (
    <div className="mx-auto w-full max-w-4xl px-6 pb-7 max-[560px]:px-4">
      {topContent ? (
        <div data-chat-input-top-content="true" className="mb-3">
          {topContent}
        </div>
      ) : null}
      <motion.form
        data-motion-component="chat-input"
        className="relative rounded-[18px] border border-line-strong bg-paper shadow-[0_2px_14px_color-mix(in_srgb,var(--ink)_7%,transparent)]"
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.2, ease: 'easeOut' }}
        onSubmit={(event) => {
          event.preventDefault()
          submit()
        }}
      >
        {slashMenuOpen ? (
          <div
            id="chat-slash-command-menu"
            data-slash-command-menu="true"
            role="listbox"
            aria-label={t('chat.commands.menu')}
            className="absolute inset-x-0 bottom-full z-20 mb-2 rounded-xl border border-line bg-paper p-1.5 shadow-[0_10px_30px_color-mix(in_srgb,var(--ink)_12%,transparent)]"
          >
            <button
              type="button"
              role="option"
              aria-selected="true"
              data-slash-command="compact"
              className="flex w-full items-center gap-2.5 rounded-lg bg-paper-hover px-3 py-2 text-left text-sm font-medium text-ink"
              onMouseDown={(event) => event.preventDefault()}
              onClick={selectCompactCommand}
            >
              <Minimize2 size={15} className="shrink-0 text-ink-soft" />
              {t('chat.commands.compact.label')}
            </button>
          </div>
        ) : null}

        {skillMenuOpen ? (
          <div
            id="chat-skill-menu"
            data-skill-menu="true"
            role="listbox"
            aria-label={t('chat.skills.menu')}
            className="absolute inset-x-0 bottom-full z-20 mb-2 max-h-64 overflow-y-auto rounded-xl border border-line bg-paper p-1.5 shadow-[0_10px_30px_color-mix(in_srgb,var(--ink)_12%,transparent)]"
          >
            {skillCandidates.map((skill, index) => {
              const selected = index === activeSkillIndex
              return (
                <button
                  ref={(element) => {
                    skillOptionRefs.current[index] = element
                  }}
                  id={`chat-skill-option-${index}`}
                  key={skill.path}
                  type="button"
                  role="option"
                  aria-selected={selected}
                  data-skill-option={skill.name}
                  className={`flex w-full items-start gap-2.5 rounded-lg px-3 py-2 text-left transition ${selected ? 'bg-paper-hover' : 'hover:bg-paper-hover/70'}`}
                  title={skill.path}
                  onMouseEnter={() => setSelectedSkillIndex(index)}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => selectSkillCandidate(skill)}
                >
                  <Puzzle size={15} className="mt-0.5 shrink-0 text-clay" />
                  <span className="min-w-0">
                    <span className="block truncate font-mono text-sm text-ink">${skill.name}</span>
                    <span className="mt-0.5 block truncate text-xs text-ink-faint">{skill.description}</span>
                  </span>
                </button>
              )
            })}
          </div>
        ) : null}

        <div className="relative">
          <SkillMentionOverlay
            value={value}
            bindings={skillBindings}
            overlayRef={overlayRef}
            contentRef={overlayContentRef}
          />
          <textarea
            ref={textareaRef}
            /*
              font-serif 覆盖 globals.css 里 `textarea { font-family: var(--font-sans) }`
              的全局规则：这一层的字是透明的，它只负责光标与选区，而两者的位置按本元素的
              字体度量算 —— 必须和上面 overlay 用同一套排版，否则光标对不齐文字。

              selection:text-transparent 同样是覆盖：globals.css 的 `::selection` 设了
              `color: var(--ink)`，会把本该透明的文字在选中时强行画出来，与 overlay 叠成
              重影。这里只保留选区底色，字仍旧由 overlay 提供。

              这两处覆盖成立的前提是 globals.css 的全局段包在 `@layer base` 里。Tailwind v4
              的工具类在 `@layer utilities`，而无层规则压过任何 cascade layer —— globals.css
              那段一旦漏在层外，这里的两个类就会被静默忽略。改动 globals.css 时留意。
            */
            className="relative z-10 max-h-48 min-h-[80px] w-full resize-none border-0 bg-transparent px-5 py-3 font-serif text-base leading-7 text-transparent caret-ink outline-none selection:bg-clay-soft/70 selection:text-transparent placeholder:text-ink-faint focus:ring-0"
            value={value}
            onChange={(event) => handleValueChange(
              event.target.value,
              event.target.selectionStart,
              event.target.selectionEnd,
            )}
            onKeyDown={handleKeyDown}
            onKeyUp={(event) => syncSelection(event.currentTarget)}
            onClick={(event) => syncSelection(event.currentTarget)}
            onFocus={(event) => {
              syncSelection(event.currentTarget)
              void onRefreshSkills?.()
            }}
            onPaste={(event) => syncSelection(event.currentTarget)}
            onCut={(event) => syncSelection(event.currentTarget)}
            onDrop={(event) => syncSelection(event.currentTarget)}
            onSelect={(event) => syncSelection(event.currentTarget)}
            onScroll={(event) => syncOverlay(event.currentTarget)}
            onCompositionStart={(event) => {
              syncSelection(event.currentTarget)
              compositionSelectionRef.current = selectionRef.current
              composingRef.current = true
            }}
            onCompositionEnd={(event) => {
              composingRef.current = false
              queueMicrotask(() => {
                compositionSelectionRef.current = null
              })
              const textarea = event.currentTarget
              syncSelection(textarea)
              window.requestAnimationFrame(() => {
                resizeTextarea(textarea)
                syncOverlay(textarea)
              })
            }}
            placeholder={t('chat.placeholder')}
            rows={2}
            disabled={disabled || isCompacting}
            aria-autocomplete="list"
            aria-controls={skillMenuOpen ? 'chat-skill-menu' : slashMenuOpen ? 'chat-slash-command-menu' : undefined}
            aria-activedescendant={skillMenuOpen ? `chat-skill-option-${activeSkillIndex}` : undefined}
            aria-expanded={skillMenuOpen || slashMenuOpen}
          />
        </div>

        <div className="flex min-h-12 items-center gap-2 px-4 py-1 sm:px-5">
          <Select
            value={permissionMode}
            onValueChange={(mode) => onPermissionModeChange(mode as RuntimePermissionMode)}
            disabled={disabled || isSending || isCompacting}
          >
            <SelectTrigger
              className="w-[clamp(96px,18vw,180px)] overflow-hidden rounded-full border border-line text-ink-soft"
              aria-label={t('chat.permissionMode')}
              title={t(`chat.permissionModes.${permissionMode}Description`)}
            >
              <ShieldCheck size={17} strokeWidth={1.8} className="shrink-0 text-ink-faint" />
              <SelectValue>
                {t(`chat.permissionModes.${permissionMode}`)}
              </SelectValue>
            </SelectTrigger>
            <SelectContent side="top" align="start">
              <SelectItem value="default">{t('chat.permissionModes.default')}</SelectItem>
              <SelectItem value="accept_edits">{t('chat.permissionModes.accept_edits')}</SelectItem>
            </SelectContent>
          </Select>

          <div className="min-w-0 flex-1" />

          <ContextUsageIndicator
            usage={contextUsage}
            breakdown={contextBreakdown}
            inspectorOpen={contextInspectorOpen}
            onInspect={onInspectContext}
          />

          <Select
            value={model}
            onValueChange={onModelChange}
            disabled={disabled || modelSelectionLocked || isSending || isCompacting || modelOptions.length === 0}
          >
            <SelectTrigger
              /* 宽度随内容自适应、设上限，短模型名完整显示，超长才截断 —— 悬停要能看到全名。 */
              className="w-auto min-w-[104px] max-w-[min(320px,34vw)] overflow-hidden rounded-full border border-line text-ink-soft"
              aria-label={t('chat.selectModel')}
              title={selectedModel ? formatModelLabel(selectedModel) : undefined}
            >
              <SelectValue placeholder={t('chat.noModel')}>
                {selectedModel ? formatModelLabel(selectedModel) : undefined}
              </SelectValue>
            </SelectTrigger>
            <SelectContent side="top" align="end">
              {modelOptions.map((option) => (
                <SelectItem key={option.modelId} value={option.modelId}>
                  {formatModelLabel(option)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <AnimatePresence initial={false} mode="wait">
            <motion.div
              key={isSending ? 'stop' : isCompacting ? 'compacting' : 'send'}
              className="shrink-0"
              initial={{ opacity: 0, scale: 0.85 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.85 }}
              transition={{ duration: 0.14 }}
            >
              {isSending ? (
                <Button size="icon" type="button" aria-label={t('chat.stop')} onClick={onCancel}>
                  <Square size={12} className="fill-current" />
                </Button>
              ) : isCompacting ? (
                <Button size="icon" type="button" aria-label={t('chat.commands.compacting')} disabled>
                  <LoaderCircle size={17} className="animate-spin" />
                </Button>
              ) : (
                <Button
                  size="icon"
                  variant="accent"
                  type="submit"
                  aria-label={t('chat.send')}
                  disabled={disabled || !model || !value.trim()}
                >
                  <ArrowUp size={18} strokeWidth={2.1} />
                </Button>
              )}
            </motion.div>
          </AnimatePresence>
        </div>
      </motion.form>
    </div>
  )
}

function formatModelLabel(model: ProviderModel): string {
  const name = model.displayName?.trim() || model.modelId
  const tier = model.modelTier.charAt(0).toUpperCase() + model.modelTier.slice(1)
  return `${name} · ${tier}`
}
