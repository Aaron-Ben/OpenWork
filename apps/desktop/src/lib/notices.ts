import { assertNever, type Notice } from "@crew/protocol";

// 通知在界面上的图标与颜色。颜色只用四种：蓝是任务的变化，绿是完成，黄是需要注意（退回、晚到），
// 紫是 Agent 自己的安排（提醒与静音）。设计稿是 out/mockups/step6-notices-mute.html。

/** 图标名都在 components/ui/icon.tsx 里画好，`<Icon name>` 的类型检查保证这一点。 */
export type NoticeIcon =
  | "clipboard"
  | "play"
  | "eye"
  | "assign"
  | "sendBack"
  | "check"
  | "closed"
  | "alarm"
  | "bell"
  | "bellOff"
  | "dot";
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
    case "mute":
      return { icon: "bellOff", tone: "violet" };
    case "unmute":
      return { icon: "bell", tone: "violet" };
    default:
      return assertNever(notice);
  }
}

/**
 * 通知那一行的文字与下面引用的说明。改状态的说明接在正文末尾（“…，@alice：标题太长”），
 * 界面把它拿出来放进引用，这一行不再重复。
 */
export function noticeParts(body: string, notice: Notice | null): { line: string; note?: string } {
  const note = notice?.type === "task.status" ? notice.note : undefined;
  const suffix = `：${note}`;
  if (!note || !body.endsWith(suffix)) return { line: body };
  return { line: body.slice(0, -suffix.length), note };
}

/** 提醒晚了这么久才触发（应用当时没在运行），卡片上写明原定的时间。与 Server 的判断一致。 */
const LATE_AFTER_MS = 60_000;

export function isLate(notice: Extract<Notice, { type: "reminder" }>, firedAt: string): boolean {
  return new Date(firedAt).getTime() - new Date(notice.dueAt).getTime() > LATE_AFTER_MS;
}
