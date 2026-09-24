import { cn } from '@/lib/utils'

export const LOCAL_USER_ID = 'local-user'

/** 头像上的字：用户是“你”，Agent 取显示名的第一个字符。 */
function initial(name: string): string {
  return Array.from(name.trim())[0]?.toUpperCase() ?? '?'
}

/**
 * 参与者头像。用户用墨色；Agent 暂用 clay 浅底，识别色在 U2e 接入（collaboration-desktop.md §11）。
 * `ring` 表示正在工作。
 */
export function ParticipantAvatar({ name, isUser, size = 32, ring = false, className }: {
  name: string
  isUser: boolean
  size?: number
  ring?: boolean
  className?: string
}) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        'grid shrink-0 place-items-center rounded-full font-bold',
        isUser ? 'bg-ink text-paper' : 'bg-clay-soft text-ink',
        ring && 'ring-2 ring-status-success ring-offset-2 ring-offset-paper',
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
            id === LOCAL_USER_ID ? 'bg-ink text-paper' : 'bg-clay-soft text-ink',
            HIVE_SPOTS[index],
          )}
        >
          {id === LOCAL_USER_ID ? you : initial(names.get(id) ?? id)}
        </span>
      ))}
    </span>
  )
}
