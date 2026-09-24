import { ShieldOff } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { RuntimeSandboxStatus } from '@/bridge/compat'

/**
 * 沙箱自检失败时常驻在输入框上方（permissions.md §3.2）：bash 已停用，直到下次启动自检通过。
 * 原因是自检的原始输出，折叠在"查看原因"里。
 */
export function SandboxUnavailableNotice({ sandbox }: { sandbox: RuntimeSandboxStatus | null }) {
  const { t } = useTranslation()
  if (sandbox?.state !== 'unavailable') return null
  return (
    <section
      role="status"
      className="mx-auto mb-3 flex w-full max-w-3xl flex-col gap-2.5 rounded-xl border border-status-warning-border bg-status-warning-soft px-4 py-3.5"
    >
      <div className="flex items-center gap-2.5">
        <ShieldOff size={18} className="shrink-0 text-status-warning-ink" aria-hidden="true" />
        <h2 className="m-0 text-sm font-semibold text-status-warning-ink">{t('chat.sandboxUnavailable.title')}</h2>
      </div>
      <p className="m-0 pl-7 text-[12.5px] leading-relaxed text-status-warning-ink">
        {t('chat.sandboxUnavailable.body')}
      </p>
      <details className="pl-7">
        <summary className="cursor-pointer text-xs text-status-warning-ink underline underline-offset-2">
          {t('chat.sandboxUnavailable.showReason')}
        </summary>
        <p className="m-0 mt-2 rounded-lg bg-paper/70 px-2.5 py-2 font-mono text-[11.5px] leading-relaxed text-ink-soft">
          {t('chat.sandboxUnavailable.reason', { reason: sandbox.reason })}
        </p>
      </details>
    </section>
  )
}
