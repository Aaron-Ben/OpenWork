import { OctagonX } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabPendingPermission } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { usePermissionStore } from './permissionStore'

export function ApprovalCard({ permission, agentName }: {
  permission: CollabPendingPermission
  agentName: string
}) {
  const { t } = useTranslation()
  const reply = usePermissionStore((state) => state.reply)
  const abort = usePermissionStore((state) => state.abort)
  const [rejecting, setRejecting] = useState(false)
  const [reason, setReason] = useState('')

  return (
    <article className="rounded-2xl border border-red-200 bg-red-50/60 p-3 dark:border-red-900 dark:bg-red-950/20">
      <p className="text-sm font-semibold">{t('collab.approvals.request', { agent: agentName, permission: permission.permission })}</p>
      {permission.patterns.length > 0 ? (
        <div className="mt-2 text-xs text-ink-muted">
          <div className="font-medium">{t('collab.approvals.paths')}</div>
          {permission.patterns.map((pattern) => <code key={pattern} className="mt-1 block break-all">{pattern}</code>)}
        </div>
      ) : null}
      {rejecting ? (
        <div className="mt-3 grid gap-2">
          <Textarea rows={2} value={reason} placeholder={t('collab.approvals.reason')} onChange={(event) => setReason(event.target.value)} />
          <Button type="button" variant="destructive" size="sm" onClick={() => void reply(permission.id, 'reject', reason.trim() || undefined)}>{t('collab.approvals.reject')}</Button>
        </div>
      ) : (
        <div className="mt-3 flex flex-wrap gap-2">
          <Button type="button" size="sm" onClick={() => void reply(permission.id, 'once')}>{t('collab.approvals.once')}</Button>
          <Button type="button" variant="outline" size="sm" onClick={() => void reply(permission.id, 'always')}>{t('collab.approvals.always')}</Button>
          <Button type="button" variant="outline" size="sm" onClick={() => setRejecting(true)}>{t('collab.approvals.reject')}</Button>
        </div>
      )}
      <Button type="button" variant="ghost" size="sm" className="mt-2 text-red-700" onClick={() => void abort(permission.id)}>
        <OctagonX size={14} />{t('collab.approvals.abort')}
      </Button>
    </article>
  )
}
