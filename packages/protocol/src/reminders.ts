import { z } from "zod";
import { assertNever } from "./assert";
import { RoomId } from "./ids";

// 提醒：Agent 给自己定的一次性或周期提醒，到点在定它的房间里写一条通知并唤醒它。
// 周期规则照 raft 只有三种（raft:packages/server/src/services/recurrence.ts），时间按本机时区。
// 取舍见 Agent Note：提醒、记忆与静音（2026-10-05-reminders-memory-mute）。

/** 每个 Agent 同时最多这么多个未触发的提醒。 */
export const REMINDER_MAX_SCHEDULED = 20;
/** 周期提醒的最短间隔（分钟）。 */
export const REMINDER_MIN_MINUTES = 5;
/** 一次性提醒最远能定到多少天以后。 */
export const REMINDER_MAX_DAYS = 365;
export const REMINDER_TITLE_MAX = 200;

export const ReminderTitle = z
  .string()
  .trim()
  .min(1, "提醒的标题不能为空")
  .max(REMINDER_TITLE_MAX, `提醒的标题最多 ${REMINDER_TITLE_MAX} 字符`);

const ClockTime = z.string().regex(/^([01]\d|2[0-3]):[0-5]\d$/, "时间要写成 HH:MM");

/** 周期规则：每隔 N 分钟、每天几点、每周几（0 是周日）几点。 */
export const ReminderRepeat = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("every"),
    minutes: z
      .number()
      .int()
      .min(REMINDER_MIN_MINUTES, `周期最短 ${REMINDER_MIN_MINUTES} 分钟`)
      .max(REMINDER_MAX_DAYS * 24 * 60),
  }),
  z.object({ kind: z.literal("daily"), time: ClockTime }),
  z.object({
    kind: z.literal("weekly"),
    days: z
      .array(z.number().int().min(0).max(6))
      .min(1, "至少选一天")
      .refine((days) => new Set(days).size === days.length, "星期不能重复"),
    time: ClockTime,
  }),
]);
export type ReminderRepeat = z.infer<typeof ReminderRepeat>;

export const ReminderStatus = z.enum(["scheduled", "fired", "canceled"]);
export type ReminderStatus = z.infer<typeof ReminderStatus>;

export const ReminderView = z.object({
  id: z.uuid(),
  roomId: RoomId,
  title: z.string(),
  /** 下一次触发的时间；已触发或已取消的一次性提醒是它原定的时间。 */
  fireAt: z.string(),
  repeat: ReminderRepeat.nullable(),
  status: ReminderStatus,
  createdAt: z.string(),
  firedAt: z.string().nullable(),
});
export type ReminderView = z.infer<typeof ReminderView>;

/** 新建提醒：给一个时间（一次性），或者给周期规则。 */
export const NewReminder = z
  .object({
    roomId: RoomId,
    title: ReminderTitle,
    at: z.iso.datetime({ offset: true }).optional(),
    repeat: ReminderRepeat.optional(),
  })
  .refine((input) => (input.at === undefined) !== (input.repeat === undefined), "at 与 repeat 恰好给一个");

/** Server 拒绝一个提醒的原因。界面显示中文；`crew` 据此写英文说明。 */
export const ReminderRefusal = z.discriminatedUnion("code", [
  z.object({ code: z.literal("reminder_limit"), max: z.number().int() }),
  z.object({ code: z.literal("reminder_past") }),
  z.object({ code: z.literal("reminder_too_far"), days: z.number().int() }),
  z.object({ code: z.literal("reminder_not_found") }),
]);
export type ReminderRefusal = z.infer<typeof ReminderRefusal>;

export function reminderRefusalText(refusal: ReminderRefusal): string {
  switch (refusal.code) {
    case "reminder_limit":
      return `最多同时有 ${refusal.max} 个未触发的提醒`;
    case "reminder_past":
      return "提醒的时间已经过去了";
    case "reminder_too_far":
      return `提醒最远只能定到 ${refusal.days} 天以后`;
    case "reminder_not_found":
      return "提醒不存在";
    default:
      return assertNever(refusal);
  }
}

const WEEKDAYS = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"] as const;

/** 周期规则的中文写法：“每 30 分钟”“每天 09:00”“每周一、周五 09:00”。 */
export function repeatText(repeat: ReminderRepeat): string {
  switch (repeat.kind) {
    case "every":
      return repeat.minutes % 60 === 0 ? `每 ${repeat.minutes / 60} 小时` : `每 ${repeat.minutes} 分钟`;
    case "daily":
      return `每天 ${repeat.time}`;
    case "weekly":
      return `每${[...repeat.days]
        .sort((a, b) => a - b)
        .map((day) => WEEKDAYS[day])
        .join("、")} ${repeat.time}`;
    default:
      return assertNever(repeat);
  }
}
