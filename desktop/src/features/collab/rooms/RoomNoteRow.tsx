import { PauseCircle, Split } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabRoomNote } from '@/bridge/collab'

/** 说明行（collaboration-desktop.md §7.3）：解释为什么有人没回复，不展示运行细节。 */
export function RoomNoteRow({ note }: { note: CollabRoomNote }) {
  const { t } = useTranslation()
  const [explaining, setExplaining] = useState(false)
  const separator = t('collab.rooms.nameSeparator')
  switch (note.kind) {
    case 'routing':
      return (
        <div className="flex items-center gap-2 pl-11 text-xs text-ink-faint">
          <Split size={14} className="shrink-0" />
          <span>{t('collab.rooms.notes.routing', { skipped: note.skippedNames.join(separator), targets: note.targetNames.join(separator) })}</span>
        </div>
      )
    case 'lap_floor':
      return (
        <div className="ml-11 flex flex-col gap-1.5 rounded-xl border border-dashed border-line-strong bg-paper-hover px-3 py-2 text-xs text-ink-soft">
          <div className="flex items-center gap-2.5">
            <PauseCircle size={15} className="shrink-0" />
            <span className="flex-1">{t('collab.rooms.notes.lapFloor', { name: note.speakerName })}</span>
            <button type="button" aria-expanded={explaining} className="font-semibold text-clay" onClick={() => setExplaining((current) => !current)}>
              {t('collab.rooms.notes.lapFloorWhy')}
            </button>
          </div>
          {explaining ? <p className="pl-6 leading-relaxed">{t('collab.rooms.notes.lapFloorExplain')}</p> : null}
        </div>
      )
    case 'loop_cap':
      return (
        <div className="ml-11 flex items-center gap-2.5 rounded-xl border border-dashed border-line-strong bg-paper-hover px-3 py-2 text-xs text-ink-soft">
          <PauseCircle size={15} className="shrink-0" />
          <span>{t('collab.rooms.notes.loopCap')}</span>
        </div>
      )
    default: {
      const unreachable: never = note
      return unreachable
    }
  }
}
