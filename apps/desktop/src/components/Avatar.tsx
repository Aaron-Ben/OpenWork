import { avatarColor, avatarInitial, ringSlots } from "../lib/avatar";
import { cn } from "../lib/cn";
import type { StatusTone } from "../lib/status";

const statusDot: Record<StatusTone, string> = {
  idle: "bg-faint",
  working: "bg-accent shadow-[0_0_0_3px_var(--accent-soft)]",
  error: "bg-danger",
};

/** Agent 的头像：彩色圆角方块加名字的第一个字，右下角可以带状态点。 */
export function AgentAvatar({
  name,
  handle,
  size = 36,
  status,
  ring = "var(--panel)",
  className,
}: {
  name: string;
  handle: string;
  size?: number;
  status?: StatusTone;
  /** 状态点外圈的颜色，与头像所在的底色一致。 */
  ring?: string;
  className?: string;
}) {
  return (
    <span
      className={cn("relative grid flex-none place-items-center font-semibold text-white", className)}
      style={{
        width: size,
        height: size,
        borderRadius: Math.round(size * 0.26),
        background: avatarColor(handle),
        fontSize: Math.round(size * 0.4),
      }}
    >
      {avatarInitial(name)}
      {status && (
        <span
          className={cn("absolute -right-0.5 -bottom-0.5 size-[11px] rounded-full border-2", statusDot[status])}
          style={{ borderColor: ring }}
        />
      )}
    </span>
  );
}

/** 本机用户的头像。 */
export function UserAvatar({ size = 36 }: { size?: number }) {
  return (
    <span
      className="grid flex-none place-items-center bg-user font-semibold text-white"
      style={{ width: size, height: size, borderRadius: Math.round(size * 0.26), fontSize: Math.round(size * 0.36) }}
    >
      你
    </span>
  );
}

/** 群聊的头像：成员的小头像围成一个环，最多 5 个。 */
export function GroupAvatar({
  members,
  size = 36,
}: {
  members: ReadonlyArray<{ name: string; handle: string }>;
  size?: number;
}) {
  const slots = ringSlots(members.length);
  return (
    <span
      className="relative block flex-none rounded-full border border-line-strong bg-hover"
      style={{ width: size, height: size }}
    >
      {slots.map((slot, index) => {
        const member = members[index];
        if (!member) return null;
        const diameter = slot.size * size;
        return (
          <span
            key={member.handle}
            className="absolute grid place-items-center rounded-full font-semibold text-white ring-[1.5px] ring-raised"
            style={{
              width: diameter,
              height: diameter,
              left: slot.x * size - diameter / 2,
              top: slot.y * size - diameter / 2,
              background: avatarColor(member.handle),
              fontSize: Math.max(7, Math.round(diameter * 0.52)),
              lineHeight: 1,
            }}
          >
            {avatarInitial(member.name)}
          </span>
        );
      })}
    </span>
  );
}
