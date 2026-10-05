import { z } from "zod";
import { AgentId, MessageId, RoomId } from "./ids";

// 运行记录：Agent 被唤醒后跑的每一轮，以及这一轮里的每一步。
// Computer 解析 Engine 的输出后上报“Engine 事件”；回复与 HELD 由 Server 在 `crew reply` 到达时记下。

/** 落单的 UTF-16 代理项：高位后面没有低位，或低位前面没有高位。 */
const LONE_SURROGATE = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g;

/**
 * 截到至多 `max` 个 UTF-16 单元，不把 emoji 这类代理对从中间切断。
 * 落单的代理项写不进 PostgreSQL 的 jsonb。
 */
export function clipText(text: string, max: number): string {
  if (text.length <= max) return text;
  const head = text.slice(0, max);
  return /[\uD800-\uDBFF]$/.test(head) ? head.slice(0, -1) : head;
}

/**
 * 能存进 PostgreSQL 的文本：text 与 jsonb 都不接受 NUL，jsonb 也不接受落单的代理项。
 * 工具输出（例如 `cat` 了一个二进制文件）与截短过的文字都可能带着它们，换成 U+FFFD。
 */
export function storableText(text: string): string {
  return text.replace(LONE_SURROGATE, "\uFFFD").replaceAll("\u0000", "\uFFFD");
}

/** 工具输入输出、模型文字这类长文本，Computer 截到这么长再上报。 */
export const RUN_TEXT_MAX = 4_096;
const Clipped = z.string().max(RUN_TEXT_MAX);

/** 一步或一轮的用量。费用的单位是美元，由 Engine 报告。 */
export const Usage = z.object({
  input: z.number().int().nonnegative(),
  output: z.number().int().nonnegative(),
  reasoning: z.number().int().nonnegative(),
  cacheRead: z.number().int().nonnegative(),
  cacheWrite: z.number().int().nonnegative(),
  cost: z.number().nonnegative(),
});
export type Usage = z.infer<typeof Usage>;

/** Engine 事件：Computer 从 Engine 的输出里解析出来，按发生顺序上报。`at` 是 ISO 8601 时间。 */
export const EngineEvent = z.discriminatedUnion("kind", [
  /** 模型开始一步。 */
  z.object({ kind: z.literal("step"), at: z.string() }),
  /** 一次工具调用，工具完成后才有。`input` 是 JSON 文本。 */
  z.object({
    kind: z.literal("tool"),
    at: z.string(),
    tool: z.string().max(100),
    title: z.string().max(500),
    input: Clipped,
    output: Clipped,
    durationMs: z.number().int().nonnegative().nullable(),
    failed: z.boolean(),
  }),
  /** 模型的文字输出。没有人看得到，只用于回看。 */
  z.object({ kind: z.literal("text"), at: z.string(), text: Clipped }),
  /** 一步结束及其用量。 */
  z.object({ kind: z.literal("step_end"), at: z.string(), usage: Usage }),
]);
export type EngineEvent = z.infer<typeof EngineEvent>;

/** 运行记录里的一步：Engine 事件，或 Server 记下的回复与 HELD。`seq` 在一轮内从 1 递增。 */
export const RunEvent = z.intersection(
  z.object({ seq: z.number().int().positive() }),
  z.discriminatedUnion("kind", [
    ...EngineEvent.options,
    /** Agent 发出了一条回复。 */
    z.object({
      kind: z.literal("reply"),
      at: z.string(),
      roomId: RoomId,
      messageId: MessageId,
      body: z.string(),
    }),
    /** Agent 的回复被 HELD 拦下：房间里有它没看过的新消息。 */
    z.object({
      kind: z.literal("held"),
      at: z.string(),
      roomId: RoomId,
      newMessages: z.number().int().positive(),
      preview: z.string(),
    }),
  ]),
);
export type RunEvent = z.infer<typeof RunEvent>;

export const RunOutcome = z.enum(["running", "succeeded", "failed", "cancelled", "interrupted"]);
export type RunOutcome = z.infer<typeof RunOutcome>;

/** 一轮被哪个房间的哪几条消息唤醒。 */
export const RunTrigger = z.object({
  roomId: RoomId,
  fromSeq: z.number().int().positive(),
  toSeq: z.number().int().positive(),
});
export type RunTrigger = z.infer<typeof RunTrigger>;

/** 列表里的一轮。 */
export const RunSummary = z.object({
  id: z.uuid(),
  agentId: AgentId,
  outcome: RunOutcome,
  /** 失败的原因；其他结果为 null。 */
  error: z.string().nullable(),
  startedAt: z.string(),
  endedAt: z.string().nullable(),
  triggers: z.array(RunTrigger),
  usage: Usage,
  steps: z.number().int().nonnegative(),
  replies: z.number().int().nonnegative(),
  holds: z.number().int().nonnegative(),
});
export type RunSummary = z.infer<typeof RunSummary>;

/** 一轮的全部内容：概要、这一轮的完整输入与按顺序的每一步。 */
export const RunDetail = RunSummary.extend({ prompt: z.string(), events: z.array(RunEvent) });
export type RunDetail = z.infer<typeof RunDetail>;
