import { assertNever, type Notice } from "@crew/protocol";

// 通知在界面上的图标与颜色。颜色只用四种：蓝是任务的变化，绿是完成，黄是需要注意（退回、晚到），
// 紫是 Agent 自己的安排（提醒）。设计稿是 out/mockups/step6-notices-mute.html。

/** 图标名都在 components/ui/icon.tsx 里画好，`<Icon name>` 的类型检查保证这一点。 */
export type NoticeIcon = "clipboard" | "play" | "eye" | "assign" | "sendBack" | "check" | "closed" | "alarm" | "dot";
export type NoticeTone = "task" | "ok" | "warn" | "violet" | "muted";

export function noticeLook(notice: Notice | null): { icon: NoticeIcon; tone: NoticeTone } {
  if (!notice) return { icon: "dot", tone: "muted" };
  switch (notice.type) {
    case "task.created":
    case "task.converted":
      return { icon: "clipboard", tone: "task" };
    case "task.claimed":
      return { icon: "play", tone: "task" };
    case "task.assigned":
      return { icon: "assign", tone: "task" };
    case "task.status":
      if (notice.sentBack) return { icon: "sendBack", tone: "warn" };
      if (notice.to === "done") return { icon: "check", tone: "ok" };
      if (notice.to === "closed") return { icon: "closed", tone: "muted" };
      if (notice.to === "in_review") return { icon: "eye", tone: "task" };
      return { icon: "play", tone: "task" };
    case "reminder":
      return { icon: "alarm", tone: "violet" };
    default:
      return assertNever(notice);
  }
}

/** 提醒晚了这么久才触发（应用当时没在运行），卡片上写明原定的时间。与 Server 的判断一致。 */
const LATE_AFTER_MS = 60_000;

export function isLate(notice: Extract<Notice, { type: "reminder" }>, firedAt: string): boolean {
  return new Date(firedAt).getTime() - new Date(notice.dueAt).getTime() > LATE_AFTER_MS;
}
