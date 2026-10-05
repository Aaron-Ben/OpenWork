import { z } from "zod";
import { AgentId, MessageId, RoomId } from "./ids";
import { TaskTag } from "./tasks";

/** 消息正文：去掉首尾空白后不能为空，最多 20,000 字符。 */
export const MESSAGE_BODY_MAX = 20_000;
export const MessageBody = z
  .string()
  .refine((body) => body.trim().length > 0, "消息正文不能为空")
  .refine((body) => body.length <= MESSAGE_BODY_MAX, `消息正文最多 ${MESSAGE_BODY_MAX} 字符`);

/** Agent 在消息里被点名用的 `@handle`：小写字母、数字与 `-`，以字母或数字开头。数据库有同样的约束。 */
export const HANDLE_MAX = 32;
export const Handle = z
  .string()
  .regex(/^[a-z0-9][a-z0-9-]*$/, "handle 只能用小写字母、数字与 -，并以字母或数字开头")
  .max(HANDLE_MAX, `handle 最多 ${HANDLE_MAX} 字符`);

/**
 * 正文里的一个 `@handle`。`@` 前面不能是字母、数字或 `_ . @ / -`，否则是邮箱或路径的一部分，例如 `a@b.com`。
 * 第 1 组是 handle，大小写不敏感，可能带着句末的 `-`。Server 据此记录点名，界面据此高亮。
 * 带 g 标志，每次调用返回新的对象：全局正则在 `exec` 之间保存位置，不能共用。
 */
export function mentionPattern(): RegExp {
  return /(?<![A-Za-z0-9_.@/-])@([A-Za-z0-9][A-Za-z0-9-]*)/g;
}

/** 把匹配到的 handle 规整为存储的写法：小写，去掉句末的 `-`（例如 “@alice-请看”）。 */
export function normalizeHandle(raw: string): string {
  return raw.toLowerCase().replace(/-+$/, "");
}

/** Server 拒绝开讨论串时的原因。界面直接显示；`crew` 据此换成给 Agent 的英文说明。 */
export const THREAD_REFUSALS = {
  direct: "私聊里不能开讨论串",
  nested: "讨论串里不能再开讨论串",
  noMessage: "消息不存在",
} as const;

/** 群聊的名字。 */
export const ROOM_NAME_MAX = 40;
export const RoomName = z
  .string()
  .trim()
  .min(1, "群聊名字不能为空")
  .max(ROOM_NAME_MAX, `群聊名字最多 ${ROOM_NAME_MAX} 字符`);

export const RoomKind = z.enum(["direct", "group"]);
export type RoomKind = z.infer<typeof RoomKind>;

/** 房间里的一个参与者：本机用户（没有 handle）或 Agent。 */
export const Participant = z.object({
  kind: z.enum(["user", "agent"]),
  id: z.string(),
  displayName: z.string(),
  handle: z.string().nullable(),
});
export type Participant = z.infer<typeof Participant>;

/**
 * Agent 当前在做什么，由 Server 从运行记录推出：有进行中的一轮是 `working`，`roomIds` 是这一轮涉及的房间；
 * 最近一轮失败、或 Computer 报告它跑不起来时是 `error`，`roomIds` 为空表示不限于某个房间。
 */
export const AgentStatus = z.discriminatedUnion("state", [
  z.object({ state: z.literal("idle") }),
  z.object({ state: z.literal("working"), runId: z.uuid(), roomIds: z.array(RoomId) }),
  z.object({ state: z.literal("error"), reason: z.string().min(1), roomIds: z.array(RoomId) }),
]);
export type AgentStatus = z.infer<typeof AgentStatus>;

// Computer 读取的响应。Computer 与 Server 是两个进程，按约定在 HTTP 边界校验。

/** `GET /computer/agents` 的一项。 */
export const ComputerAgent = z.object({
  id: AgentId,
  displayName: z.string(),
  handle: z.string(),
  persona: z.string(),
  engineId: z.string(),
  model: z.string(),
  roomId: RoomId,
});
export type ComputerAgent = z.infer<typeof ComputerAgent>;

/**
 * 一条消息。界面读取房间与 Agent 读取 inbox 共用。`kind` 是 `system` 时是通知（例如“领取了 #3”），
 * 作者是做这件事的人。
 */
export const MessageView = z.object({
  id: MessageId,
  seq: z.number().int().positive(),
  kind: z.enum(["text", "system"]),
  author: Participant,
  body: z.string(),
  createdAt: z.string(),
});
export type MessageView = z.infer<typeof MessageView>;

/** inbox 里的消息多两个字段：它是否 @ 了读取 inbox 的这个 Agent，以及它是不是一个任务的宿主消息。 */
export const InboxMessage = MessageView.extend({ mentionsYou: z.boolean(), task: TaskTag.nullable() });
export type InboxMessage = z.infer<typeof InboxMessage>;

/**
 * inbox 的一项：一个房间里已读位置之后的消息，以及房间的名字与成员。
 * 讨论串也是一项：名字与成员是它所在群聊的，`parent` 是它所在的群聊与挂在下面的那条消息。
 */
export const InboxRoom = z.object({
  roomId: RoomId,
  kind: z.enum(["direct", "group", "thread"]),
  /** 群聊的名字（讨论串是它所在群聊的名字）；私聊为 null。 */
  name: z.string().nullable(),
  members: z.array(Participant),
  parent: z.object({ roomId: RoomId, message: MessageView.extend({ task: TaskTag.nullable() }) }).nullable(),
  messages: z.array(InboxMessage).min(1),
});
export type InboxRoom = z.infer<typeof InboxRoom>;

/**
 * `crew reply` 的结果：发出了，或者被 HELD 拦下。
 * 拦下时附上 Agent 还没看到的、别人发的新消息（从最早的开始，至多若干条），
 * `omitted` 是这之后还没有附上的条数：Agent 再次回复时接着返回。
 */
export const ReplyOutcome = z.discriminatedUnion("outcome", [
  /** `roomId` 是消息实际所在的房间：在一条消息下发言时是那条消息的讨论串。 */
  z.object({ outcome: z.literal("posted"), id: MessageId, roomId: RoomId, seq: z.number().int().positive() }),
  z.object({
    outcome: z.literal("held"),
    roomId: RoomId,
    newMessages: z.array(MessageView).min(1),
    omitted: z.number().int().nonnegative(),
  }),
]);
export type ReplyOutcome = z.infer<typeof ReplyOutcome>;

// SSE 只传失效提示，不传业务正文。收到提示的一方重新读取对应的数据。

export const DesktopEvent = z.discriminatedUnion("type", [
  /** 这个房间有新消息。讨论串有新消息时，它所在的群聊也收到一条：讨论串的摘要变了。 */
  z.object({ type: z.literal("room.messages"), roomId: RoomId }),
  /** Agent 列表或某个 Agent 的状态变了。 */
  z.object({ type: z.literal("agents") }),
  /** 群聊列表或某个群聊的成员变了。 */
  z.object({ type: z.literal("rooms") }),
  /** 这一轮有了新的一步或结束了。`roomIds` 是这一轮涉及的房间。 */
  z.object({ type: z.literal("run.activity"), runId: z.uuid(), roomIds: z.array(RoomId) }),
  /** Computer 上报了新的可用模型列表。 */
  z.object({ type: z.literal("models") }),
]);
export type DesktopEvent = z.infer<typeof DesktopEvent>;

export const ComputerEvent = z.discriminatedUnion("type", [
  /** 这个 Agent 可能有新消息。 */
  z.object({ type: z.literal("agent.wake"), agentId: AgentId }),
  /** Agent 列表变了。 */
  z.object({ type: z.literal("agents") }),
]);
export type ComputerEvent = z.infer<typeof ComputerEvent>;
