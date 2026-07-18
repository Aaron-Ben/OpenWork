import { useEffect, useRef, type ReactNode } from 'react'
import { ArrowUp, ShieldCheck, Square } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { ProviderModel } from '../../models/contracts'

interface ChatInputProps {
  model: string
  modelOptions: ProviderModel[]
  modelSelectionLocked?: boolean
  value: string
  isSending: boolean
  disabled?: boolean
  onValueChange: (value: string) => void
  onModelChange: (model: string) => void
  onSubmit: () => void
  onCancel?: () => void
  topContent?: ReactNode
}

export function ChatInput({
  model,
  modelOptions,
  modelSelectionLocked = false,
  value,
  isSending,
  disabled = false,
  onValueChange,
  onModelChange,
  onSubmit,
  onCancel,
  topContent,
}: ChatInputProps) {
  const { t } = useTranslation()
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const composingRef = useRef(false)
  const selectedModel = modelOptions.find((option) => option.modelId === model)

  useEffect(() => {
    const textarea = textareaRef.current
    if (!textarea) return
    textarea.style.height = 'auto'
    textarea.style.height = `${Math.min(textarea.scrollHeight, 200)}px`
  }, [value])

  function handleKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (composingRef.current || event.nativeEvent.isComposing || event.keyCode === 229) return
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault()
      onSubmit()
    }
  }

  return (
    <div className="mx-auto w-full max-w-[980px] px-4 pb-7">
      {topContent ? (
        <div data-chat-input-top-content="true" className="mb-3">
          {topContent}
        </div>
      ) : null}
      <motion.form
        data-motion-component="chat-input"
        className="overflow-hidden rounded-[18px] border border-line bg-paper shadow-[0_18px_60px_rgba(31,30,29,0.10)]"
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.2, ease: 'easeOut' }}
        onSubmit={(event) => {
          event.preventDefault()
          onSubmit()
        }}
      >
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
          disabled={disabled}
        />

        <div className="mx-5 border-t border-line" />

        <div className="flex min-h-12 items-center gap-2 px-4 py-1 sm:px-5">
          <div
            className="flex h-8 items-center gap-2 px-1.5 text-sm font-medium text-ink-faint"
            aria-label={t('chat.approvalMode', { mode: t('chat.askForApproval') })}
            title={t('chat.askForApprovalDescription')}
          >
            <ShieldCheck size={17} strokeWidth={1.8} />
            <span className="max-[420px]:hidden">{t('chat.askForApproval')}</span>
          </div>

          <div className="min-w-0 flex-1" />

          <Select
            value={model}
            onValueChange={onModelChange}
            disabled={disabled || modelSelectionLocked || isSending || modelOptions.length === 0}
          >
            <SelectTrigger className="w-[clamp(110px,28vw,260px)]" aria-label={t('chat.selectModel')}>
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
              key={isSending ? 'stop' : 'send'}
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
