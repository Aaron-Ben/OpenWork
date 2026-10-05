import { z } from "zod";
import { assertNever } from "./assert";
import { RoomId } from "./ids";

// 静音：Agent 让一个群的消息不再唤醒它，@ 它的、它关注的讨论串、分配给它的任务与它自己的提醒照常。
// 只能 Agent 自己静音，用户只能解除。取舍见 Agent Note：提醒、记忆与静音（2026-10-05-reminders-memory-mute）。

/** 定时静音的最短与最长时长（分钟）。不给时长就一直静音，直到解除。 */
export const MUTE_MIN_MINUTES = 15;
export const MUTE_MAX_MINUTES = 7 * 24 * 60;

/** 一个 Agent 在一个群里的静音状态。`until` 为 null 且 `muted` 为 true 是一直静音。 */
export const MuteState = z.object({
  roomId: RoomId,
  muted: z.boolean(),
  until: z.string().nullable(),
});
export type MuteState = z.infer<typeof MuteState>;

export const NewMute = z.object({
  roomId: RoomId,
  minutes: z
    .number()
    .int()
    .min(MUTE_MIN_MINUTES, `静音最短 ${MUTE_MIN_MINUTES} 分钟`)
    .max(MUTE_MAX_MINUTES, "静音最长 7 天")
    .optional(),
});

/** Server 拒绝静音的原因。界面显示中文；`crew` 据此写英文说明。 */
export const MuteRefusal = z.discriminatedUnion("code", [
  z.object({ code: z.literal("mute_direct") }),
  z.object({ code: z.literal("mute_thread") }),
]);
export type MuteRefusal = z.infer<typeof MuteRefusal>;

export function muteRefusalText(refusal: MuteRefusal): string {
  switch (refusal.code) {
    case "mute_direct":
      return "私聊不能静音";
    case "mute_thread":
      return "讨论串不能单独静音，静音它所在的群聊";
    default:
      return assertNever(refusal);
  }
}
