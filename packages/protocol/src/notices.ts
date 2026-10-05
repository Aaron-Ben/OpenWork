import { z } from "zod";
import { ReminderRepeat } from "./reminders";
import { TaskStatus } from "./tasks";

// 通知（`kind = 'system'` 的消息）的类型与数据。界面据此画图标与提醒卡片，不从文字里解析；
// 正文照旧保留，Agent 读到的仍是文字。设计稿是 apps/desktop/out/mockups/step6-notices-mute.html。

export const Notice = z.discriminatedUnion("type", [
  /** 新建任务（标题消息变成任务）。 */
  z.object({ type: z.literal("task.created"), number: z.number().int(), assignee: z.string().nullable() }),
  /** 把已有的消息转成任务。 */
  z.object({ type: z.literal("task.converted"), number: z.number().int(), assignee: z.string().nullable() }),
  z.object({ type: z.literal("task.claimed"), number: z.number().int() }),
  /** 改状态。`sentBack` 是别人把负责人的任务退回，通知 @ 了负责人。 */
  z.object({
    type: z.literal("task.status"),
    number: z.number().int(),
    from: TaskStatus,
    to: TaskStatus,
    sentBack: z.boolean(),
  }),
  /** 换负责人；`assignee` 为 null 是取消负责人。 */
  z.object({ type: z.literal("task.assigned"), number: z.number().int(), assignee: z.string().nullable() }),
  /** 提醒到点。`setAt` 是定提醒的时间，`dueAt` 是这一次原定的时间。 */
  z.object({
    type: z.literal("reminder"),
    title: z.string(),
    repeat: ReminderRepeat.nullable(),
    setAt: z.string(),
    dueAt: z.string(),
  }),
]);
export type Notice = z.infer<typeof Notice>;
