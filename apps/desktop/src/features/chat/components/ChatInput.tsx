import { useEffect, useRef, useState, type ReactNode } from 'react'
import { ArrowUp, LoaderCircle, Minimize2, ShieldCheck, Square } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { ProviderModel } from '../../models/contracts'
import type { RuntimePermissionMode } from '../../../bridge/compat'
import type { ContextUsage } from '../contextUsage'
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
  onValueChange: (value: string) => void
  onModelChange: (model: string) => void
  onPermissionModeChange: (mode: RuntimePermissionMode) => void
  onSubmit: () => void
  onCancel?: () => void
  topContent?: ReactNode
  contextUsage?: ContextUsage | null
  contextInspectorOpen?: boolean
  onInspectContext?: () => void
  onSlashCommand?: (command: ChatSlashCommand) => void
}

export type ChatSlashCommand = 'compact'

export function ChatInput({
  model,
  modelOptions,
  modelSelectionLocked = false,
  permissionMode,
  value,
  isSending,
  isCompacting = false,
  disabled = false,
  onValueChange,
  onModelChange,
  onPermissionModeChange,
  onSubmit,
  onCancel,
  topContent,
  contextUsage,
  contextInspectorOpen = false,
  onInspectContext,
  onSlashCommand,
}: ChatInputProps) {
  const { t } = useTranslation()
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const composingRef = useRef(false)
  const [slashMenuDismissed, setSlashMenuDismissed] = useState(false)
  const selectedModel = modelOptions.find((option) => option.modelId === model)
  const slashQuery = value.startsWith('/') && !/\s/.test(value)
    ? value.slice(1).toLowerCase()
    : null
  const compactMatches = slashQuery !== null && 'compact'.startsWith(slashQuery)
  const slashMenuOpen = compactMatches
    && !slashMenuDismissed
    && !disabled
    && !isSending
    && !isCompacting

  useEffect(() => {
    const textarea = textareaRef.current
    if (!textarea) return
    textarea.style.height = 'auto'
    textarea.style.height = `${Math.min(textarea.scrollHeight, 200)}px`
  }, [value])

  useEffect(() => setSlashMenuDismissed(false), [value])

  function selectCompactCommand() {
    if (onSlashCommand) {
      onSlashCommand('compact')
    } else {
      onValueChange('/compact')
    }
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (composingRef.current || event.nativeEvent.isComposing || event.keyCode === 229) return
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
      onSubmit()
    }
  }

  return (
    <div className="mx-auto w-full max-w-3xl px-6 pb-7 max-[560px]:px-4">
      {topContent ? (
        <div data-chat-input-top-content="true" className="mb-3">
          {topContent}
        </div>
      ) : null}
      <motion.form
        data-motion-component="chat-input"
        className="relative rounded-[18px] border border-line bg-paper shadow-[0_18px_60px_rgba(31,30,29,0.10)]"
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.2, ease: 'easeOut' }}
        onSubmit={(event) => {
          event.preventDefault()
          onSubmit()
        }}
      >
        {slashMenuOpen ? (
          <div
            id="chat-slash-command-menu"
            data-slash-command-menu="true"
            role="listbox"
            aria-label={t('chat.commands.menu')}
            className="absolute inset-x-0 bottom-full z-20 mb-2 rounded-xl border border-line bg-paper p-1.5 shadow-[0_14px_36px_rgba(31,30,29,0.14)]"
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

        <textarea
          ref={textareaRef}
          className="max-h-48 min-h-[80px] w-full resize-none border-0 bg-transparent px-5 py-3 text-base leading-7 text-ink outline-none placeholder:text-ink-faint focus:ring-0"
          value={value}
          onChange={(event) => onValueChange(event.target.value)}
          onKeyDown={handleKeyDown}
          onCompositionStart={() => {
            composingRef.current = true
          }}
          onCompositionEnd={() => {
            composingRef.current = false
          }}
          placeholder={t('chat.placeholder')}
          rows={2}
          disabled={disabled || isCompacting}
          aria-autocomplete="list"
          aria-controls={slashMenuOpen ? 'chat-slash-command-menu' : undefined}
          aria-expanded={slashMenuOpen}
        />

        <div className="mx-5 border-t border-line" />

        <div className="flex min-h-12 items-center gap-2 px-4 py-1 sm:px-5">
          <Select
            value={permissionMode}
            onValueChange={(mode) => onPermissionModeChange(mode as RuntimePermissionMode)}
            disabled={disabled || isSending || isCompacting}
          >
            <SelectTrigger
              className="w-[clamp(96px,18vw,180px)] overflow-hidden"
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
            inspectorOpen={contextInspectorOpen}
            onInspect={onInspectContext}
          />

          <Select
            value={model}
            onValueChange={onModelChange}
            disabled={disabled || modelSelectionLocked || isSending || isCompacting || modelOptions.length === 0}
          >
            <SelectTrigger className="w-[clamp(104px,20vw,200px)] overflow-hidden" aria-label={t('chat.selectModel')}>
              <SelectValue className="min-w-0 flex-1 truncate text-left" placeholder={t('chat.noModel')}>
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
