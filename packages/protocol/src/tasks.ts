import { z } from "zod";
import { assertNever } from "./assert";
import { AgentId, MessageId, RoomId } from "./ids";

// 任务：房间里一条消息变成的待办。状态固定，流转按 raft 的允许表
// （raft:packages/server/src/services/taskService.ts 的 VALID_TRANSITIONS）。取舍见 Agent Note：任务（2026-10-05-tasks）。

export const TASK_STATUSES = ["todo", "in_progress", "in_review", "done", "closed"] as const;
export const TaskStatus = z.enum(TASK_STATUSES);
export type TaskStatus = z.infer<typeof TaskStatus>;

/** 每种状态可以改成哪些状态。关闭是“不做了”，任何时候都能关；完成与关闭都能重新打开。 */
export const TASK_TRANSITIONS: Record<TaskStatus, readonly TaskStatus[]> = {
  todo: ["in_progress", "closed"],
  in_progress: ["in_review", "done", "closed"],
  in_review: ["done", "in_progress", "closed"],
  done: ["todo", "in_progress", "in_review", "closed"],
  closed: ["todo", "in_progress"],
};

/** 退回：从待审或完成回到要继续做的状态。别人退回时 @ 负责人，让它被唤醒。 */
export function isSendBack(from: TaskStatus, to: TaskStatus): boolean {
  return (from === "in_review" || from === "done") && (to === "in_progress" || to === "todo");
}

/** 状态的中文名，界面与通知共用。 */
export function taskStatusLabel(status: TaskStatus): string {
  switch (status) {
    case "todo":
      return "待办";
    case "in_progress":
      return "进行中";
    case "in_review":
      return "待审";
    case "done":
      return "完成";
    case "closed":
      return "关闭";
    default:
      return assertNever(status);
  }
}

/** 任务标题：去掉首尾空白后 1 到 200 字符。转成任务时取消息正文的第一行，太长时截短。 */
export const TASK_TITLE_MAX = 200;
export const TaskTitle = z
  .string()
  .trim()
  .min(1, "任务标题不能为空")
  .max(TASK_TITLE_MAX, `任务标题最多 ${TASK_TITLE_MAX} 字符`);

/** 改状态时的一句说明，写进通知：退回时写要改什么，改成待审时写做了什么。 */
export const TASK_NOTE_MAX = 2000;
export const TaskNote = z.string().trim().min(1, "说明不能为空").max(TASK_NOTE_MAX, `说明最多 ${TASK_NOTE_MAX} 字符`);

/** 界面与 Agent 看到的一个任务。`threadId` 是宿主消息的讨论串；私聊里的任务没有讨论串。 */
export const TaskView = z.object({
  id: z.uuid(),
  roomId: RoomId,
  number: z.number().int().positive(),
  title: z.string(),
  status: TaskStatus,
  assignee: z.object({ id: AgentId, displayName: z.string(), handle: z.string() }).nullable(),
  messageId: MessageId,
  threadId: RoomId.nullable(),
  createdAt: z.string(),
  updatedAt: z.string(),
});
export type TaskView = z.infer<typeof TaskView>;

/** 本轮输入里宿主消息后面的任务后缀用到的信息。 */
export const TaskTag = z.object({
  number: z.number().int().positive(),
  status: TaskStatus,
  assignee: z.string().nullable(),
});
export type TaskTag = z.infer<typeof TaskTag>;

/**
 * Server 拒绝一次任务操作的原因。界面显示 `taskRefusalText` 的中文；`crew` 据此写给 Agent 的英文说明。
 */
export const TaskRefusal = z.discriminatedUnion("code", [
  z.object({ code: z.literal("not_found"), number: z.number().int() }),
  z.object({ code: z.literal("message_not_found") }),
  z.object({ code: z.literal("system_message") }),
  z.object({ code: z.literal("in_thread") }),
  z.object({ code: z.literal("already_task"), number: z.number().int() }),
  z.object({ code: z.literal("not_member"), handle: z.string() }),
  z.object({ code: z.literal("claimed"), number: z.number().int(), by: z.string() }),
  z.object({ code: z.literal("not_claimable"), number: z.number().int(), status: TaskStatus }),
  z.object({ code: z.literal("transition"), number: z.number().int(), from: TaskStatus, to: TaskStatus }),
  z.object({ code: z.literal("needs_assignee"), number: z.number().int() }),
  z.object({ code: z.literal("finished"), number: z.number().int(), status: TaskStatus }),
  z.object({ code: z.literal("changed"), number: z.number().int() }),
]);
export type TaskRefusal = z.infer<typeof TaskRefusal>;

export function taskRefusalText(refusal: TaskRefusal): string {
  switch (refusal.code) {
    case "not_found":
      return `任务 #${refusal.number} 不存在`;
    case "message_not_found":
      return "消息不存在";
    case "system_message":
      return "通知不能转成任务";
    case "in_thread":
      return "讨论串里的消息不能转成任务";
    case "already_task":
      return `这条消息已经是任务 #${refusal.number}`;
    case "not_member":
      return `@${refusal.handle} 不在这个房间里`;
    case "claimed":
      return `任务 #${refusal.number} 已经由 @${refusal.by} 负责`;
    case "not_claimable":
      return `任务 #${refusal.number} 是${taskStatusLabel(refusal.status)}，不能领取`;
    case "transition":
      return `任务 #${refusal.number} 不能从${taskStatusLabel(refusal.from)}改成${taskStatusLabel(refusal.to)}`;
    case "needs_assignee":
      return `任务 #${refusal.number} 还没有负责人，先分配或领取`;
    case "finished":
      return `任务 #${refusal.number} 已经${taskStatusLabel(refusal.status)}，不能再分配`;
    case "changed":
      return `任务 #${refusal.number} 刚被别人改过，请刷新后再试`;
    default:
      return assertNever(refusal);
  }
}
