import { cn } from '@/lib/utils'
import { identityClasses } from './agentIdentity'

export const LOCAL_USER_ID = 'local-user'

/** 头像上的字：用户是“你”，Agent 取显示名的第一个字符。 */
function initial(name: string): string {
  return Array.from(name.trim())[0]?.toUpperCase() ?? '?'
}

/**
 * 参与者头像：用户用墨色，Agent 用自己的识别色（collaboration-desktop.md §11）。`ring` 表示正在工作，
 * 外圈也是识别色。
 */
export function ParticipantAvatar({ participantId, name, size = 32, ring = false, className }: {
  participantId: string
  name: string
  size?: number
  ring?: boolean
  className?: string
}) {
  const isUser = participantId === LOCAL_USER_ID
  const identity = identityClasses(participantId)
  return (
    <span
      aria-hidden="true"
      className={cn(
        'grid shrink-0 place-items-center rounded-full font-bold',
        isUser ? 'bg-ink text-paper' : identity.avatar,
        ring && ['ring-2 ring-offset-2 ring-offset-paper', identity.ring],
        className,
      )}
      style={{ width: size, height: size, fontSize: Math.round(size * 0.4) }}
    >
      {initial(name)}
    </span>
  )
}

const HIVE_SPOTS = ['left-0 top-0', 'right-0 top-0.5', 'bottom-0 left-1', 'bottom-px right-px']

/** 群组头像：最多四个成员的小头像拼成一簇，用户排在最前（collaboration-desktop.md §7.1）。 */
export function RoomHive({ memberIds, names, you }: {
  memberIds: string[]
  names: ReadonlyMap<string, string>
  you: string
}) {
  return (
    <span aria-hidden="true" className="relative size-[38px] shrink-0">
      {memberIds.slice(0, HIVE_SPOTS.length).map((id, index) => (
        <span
          key={id}
          className={cn(
            'absolute grid size-[19px] place-items-center rounded-full border-2 border-paper-hover text-[9px] font-bold',
            id === LOCAL_USER_ID ? 'bg-ink text-paper' : identityClasses(id).avatar,
            HIVE_SPOTS[index],
          )}
        >
          {id === LOCAL_USER_ID ? you : initial(names.get(id) ?? id)}
        </span>
      ))}
    </span>
  )
}
