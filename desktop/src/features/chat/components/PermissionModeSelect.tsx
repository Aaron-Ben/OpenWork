import { Pencil, ShieldCheck } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { RuntimePermissionMode } from '@/bridge/compat'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'

interface PermissionModeSelectProps {
  mode: RuntimePermissionMode
  disabled: boolean
  onChange: (mode: RuntimePermissionMode) => void
}

const MODES: { value: RuntimePermissionMode; code: string; Icon: typeof ShieldCheck }[] = [
  { value: 'auto', code: 'auto', Icon: ShieldCheck },
  { value: 'accept_edits', code: 'accept-edits', Icon: Pencil },
]

function isPermissionMode(value: string): value is RuntimePermissionMode {
  return MODES.some((mode) => mode.value === value)
}

/**
 * 输入框旁常驻的模式指示器（permissions.md §6.1）。Turn 运行中也能切换：新模式从
 * 下一次工具调用起生效，由 Core 保证。
 */
export function PermissionModeSelect({ mode, disabled, onChange }: PermissionModeSelectProps) {
  const { t } = useTranslation()
  const Icon = MODES.find((entry) => entry.value === mode)?.Icon ?? ShieldCheck
  return (
    <Select
      value={mode}
      onValueChange={(value) => {
        if (isPermissionMode(value)) onChange(value)
      }}
      disabled={disabled}
    >
      <SelectTrigger
        className="w-[clamp(96px,22vw,200px)] rounded-full border border-line text-ink-soft data-[state=open]:border-clay data-[state=open]:bg-clay-soft data-[state=open]:text-ink"
        aria-label={t('chat.permissionModeLabel', { mode: t(`chat.permissionModes.${mode}`) })}
        title={t(`chat.permissionModes.${mode}Description`)}
      >
        <Icon size={16} strokeWidth={1.8} className="shrink-0 text-clay" />
        <SelectValue>{t(`chat.permissionModes.${mode}`)}</SelectValue>
      </SelectTrigger>
      <SelectContent side="top" align="start" className="w-[min(560px,90vw)]">
        {MODES.map(({ value, code, Icon: ItemIcon }) => (
          <SelectItem key={value} value={value} className="items-start py-3 data-[state=checked]:bg-clay-soft">
            <span className="flex items-start gap-3">
              <ItemIcon size={18} className="mt-0.5 shrink-0 text-clay" />
              <span className="flex flex-col gap-1">
                <span className="flex items-center gap-2">
                  <span className="text-sm font-semibold text-ink">{t(`chat.permissionModes.${value}`)}</span>
                  <code className="font-mono text-[11px] text-ink-soft">{code}</code>
                  {value === 'auto' ? (
                    <span className="text-[11px] text-ink-soft">{t('chat.permissionModes.defaultTag')}</span>
                  ) : null}
                </span>
                <span className="whitespace-normal text-xs leading-relaxed text-ink-soft">
                  {t(`chat.permissionModes.${value}Description`)}
                </span>
              </span>
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
