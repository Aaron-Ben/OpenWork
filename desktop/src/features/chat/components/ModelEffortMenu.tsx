import { Check, ChevronRight, ChevronUp } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

export interface ModelOption {
  /** Model reference `<providerId>/<modelId>`. */
  ref: string
  label: string
  reasoningEfforts: string[]
}

interface ModelEffortMenuProps {
  options: ModelOption[]
  modelRef: string | null
  /** The effort that the next request sends; null when the model does not reason. */
  reasoningEffort: string | null
  disabled?: boolean
  onModelChange: (ref: string) => void
  onReasoningEffortChange: (effort: string) => void
}

const rowClass =
  'flex w-full cursor-pointer items-center gap-3 px-3 py-2 text-sm text-ink data-[highlighted]:bg-paper-hover data-[state=open]:bg-paper-hover'

/** Two-level selector: model, then reasoning effort, as in DSH. */
export function ModelEffortMenu({
  options,
  modelRef,
  reasoningEffort,
  disabled = false,
  onModelChange,
  onReasoningEffortChange,
}: ModelEffortMenuProps) {
  const { t } = useTranslation()
  const selected = options.find((option) => option.ref === modelRef)
  const modelLabel = selected?.label ?? modelRef ?? t('chat.noModel')
  const effortLabel = reasoningEffort ? reasoningEffortLabel(t, reasoningEffort) : null

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        disabled={disabled || options.length === 0}
        aria-label={t('chat.selectModel')}
        title={effortLabel ? `${modelLabel} · ${effortLabel}` : modelLabel}
        className="flex max-w-[min(320px,34vw)] min-w-0 items-center gap-1.5 rounded-full border border-line px-3 py-1.5 text-sm text-ink-soft outline-none hover:bg-paper-hover disabled:opacity-60"
      >
        <span className="truncate">{modelLabel}</span>
        {effortLabel ? <span className="shrink-0 text-ink-faint">{effortLabel}</span> : null}
        <ChevronUp size={14} className="shrink-0 text-ink-faint" />
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="end" className="min-w-64">
        <DropdownMenuSub>
          <DropdownMenuSubTrigger className={rowClass}>
            <span className="flex-1">{t('chat.model')}</span>
            <span className="max-w-40 truncate text-ink-faint">{modelLabel}</span>
            <ChevronRight size={14} className="text-ink-faint" />
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent>
            {options.map((option) => (
              <DropdownMenuItem
                key={option.ref}
                className={rowClass}
                onSelect={() => onModelChange(option.ref)}
              >
                <span className="flex-1 truncate">{option.label}</span>
                {option.ref === modelRef ? <Check size={14} className="text-clay" /> : null}
              </DropdownMenuItem>
            ))}
          </DropdownMenuSubContent>
        </DropdownMenuSub>
        {selected && selected.reasoningEfforts.length > 0 ? (
          <DropdownMenuSub>
            <DropdownMenuSubTrigger className={rowClass}>
              <span className="flex-1">{t('chat.reasoningEffort')}</span>
              <span className="text-ink-faint">{effortLabel}</span>
              <ChevronRight size={14} className="text-ink-faint" />
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              {selected.reasoningEfforts.map((effort) => (
                <DropdownMenuItem
                  key={effort}
                  className={rowClass}
                  onSelect={() => onReasoningEffortChange(effort)}
                >
                  <span className="flex-1">{reasoningEffortLabel(t, effort)}</span>
                  {effort === reasoningEffort ? <Check size={14} className="text-clay" /> : null}
                </DropdownMenuItem>
              ))}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        ) : null}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** Effort values are vendor strings; known ones get a translated label, others show as-is. */
export function reasoningEffortLabel(
  t: (key: string, options?: { defaultValue: string }) => string,
  effort: string,
): string {
  return t(`chat.reasoningEfforts.${effort}`, { defaultValue: effort })
}
