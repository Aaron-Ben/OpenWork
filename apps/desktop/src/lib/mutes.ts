import type { AgentId, DesktopGroup } from "@crew/protocol";
import { formatMessageTime } from "./time";

// Agent 在群里的静音。只有 Agent 自己能静音，用户在界面上只能解除。取舍见 Agent Note：提醒、记忆与静音
// （2026-10-05-reminders-memory-mute）决策 9、10；设计稿是 out/mockups/step6-notices-mute.html。

export type Mute = DesktopGroup["mutes"][number];

/** 群里现在静音着的 Agent。Server 列出时没到期、界面开着期间到期的也去掉：到期不发通知，界面自己判断。 */
export function activeMutes(group: DesktopGroup, now: Date): Map<AgentId, Mute> {
  return new Map(
    group.mutes.filter((mute) => mute.until === null || new Date(mute.until) > now).map((mute) => [mute.agentId, mute]),
  );
}

/** 静音到什么时候：“今天 18:00”“10月6日 18:00”；一直静音时是“直到解除”。 */
export function muteUntilText(mute: Mute, now: Date): string {
  if (mute.until === null) return "直到解除";
  const time = formatMessageTime(mute.until, now);
  return new Date(mute.until).toDateString() === now.toDateString() ? `今天 ${time}` : time;
}

/** 静音时仍会叫醒它的四种消息。 */
export const STILL_WAKES = ["@ 它的消息", "它关注的讨论串", "分配给它的任务", "它自己的提醒"] as const;
