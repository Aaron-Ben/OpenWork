import { z } from "zod";
import { AgentId, MessageId, RoomId } from "./ids";

/** 消息正文：去掉首尾空白后不能为空，最多 20,000 字符。 */
export const MESSAGE_BODY_MAX = 20_000;
export const MessageBody = z
  .string()
  .refine((body) => body.trim().length > 0, "消息正文不能为空")
  .refine((body) => body.length <= MESSAGE_BODY_MAX, `消息正文最多 ${MESSAGE_BODY_MAX} 字符`);

/** Agent 当前在做什么。只存在 Server 内存中，由 Computer 上报。 */
export const AgentStatus = z.discriminatedUnion("state", [
  z.object({ state: z.literal("idle") }),
  z.object({ state: z.literal("working") }),
  z.object({ state: z.literal("error"), reason: z.string().min(1) }),
]);
export type AgentStatus = z.infer<typeof AgentStatus>;

// Computer 读取的响应。Computer 与 Server 是两个进程，按约定在 HTTP 边界校验。

/** `GET /computer/agents` 的一项。 */
export const ComputerAgent = z.object({
  id: AgentId,
  displayName: z.string(),
  persona: z.string(),
  engineId: z.string(),
  model: z.string(),
  roomId: RoomId,
});
export type ComputerAgent = z.infer<typeof ComputerAgent>;

export const InboxMessage = z.object({
  id: MessageId,
  seq: z.number().int().positive(),
  author: z.object({ kind: z.enum(["user", "agent"]), id: z.string(), displayName: z.string() }),
  body: z.string(),
  createdAt: z.string(),
});
export type InboxMessage = z.infer<typeof InboxMessage>;

/** `GET /computer/agents/:id/inbox` 的一项：一个房间里已读位置之后的消息。 */
export const InboxRoom = z.object({
  roomId: RoomId,
  kind: z.literal("direct"),
  messages: z.array(InboxMessage).min(1),
});
export type InboxRoom = z.infer<typeof InboxRoom>;

// SSE 只传失效提示，不传业务正文。收到提示的一方重新读取对应的数据。

export const DesktopEvent = z.discriminatedUnion("type", [
  /** 这个房间有新消息。 */
  z.object({ type: z.literal("room.messages"), roomId: RoomId }),
  /** Agent 列表或某个 Agent 的状态变了。 */
  z.object({ type: z.literal("agents") }),
]);
export type DesktopEvent = z.infer<typeof DesktopEvent>;

export const ComputerEvent = z.discriminatedUnion("type", [
  /** 这个 Agent 可能有新消息。 */
  z.object({ type: z.literal("agent.wake"), agentId: AgentId }),
  /** Agent 列表变了。 */
  z.object({ type: z.literal("agents") }),
]);
export type ComputerEvent = z.infer<typeof ComputerEvent>;
