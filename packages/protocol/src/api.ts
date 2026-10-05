import { z } from "zod";
import {
  AgentStatus,
  ComputerAgent,
  Handle,
  InboxRoom,
  MessageBody,
  MessageView,
  Participant,
  ReplyOutcome,
  RoomKind,
  RoomName,
} from "./collab";
import { AgentId, MessageId, RoomId } from "./ids";
import { NewReminder, ReminderRefusal, ReminderView } from "./reminders";
import { EngineEvent, RunDetail, RunSummary, RunTrigger } from "./runs";
import { TaskRefusal, TaskStatus, TaskTitle, TaskView } from "./tasks";

// Server 的 HTTP 接口契约：每个接口的方法、路径、参数、请求体与响应的 schema。
// Server 按它注册路由并校验输入，返回值必须符合响应 schema 的类型；客户端按它发请求并校验响应。
// 改一个响应字段时改这里，Server 与全部客户端一起在类型检查中报错。

export interface Endpoint {
  readonly method: "GET" | "POST";
  /** Express 风格的路径，`:name` 是路径参数。 */
  readonly path: string;
  readonly params?: z.ZodType;
  /** 查询参数。值在 URL 里是字符串，schema 负责转换。 */
  readonly query?: z.ZodType;
  readonly body?: z.ZodType;
  /** 没有响应体的接口不写，返回 204。 */
  readonly response?: z.ZodType;
  /** 成功时的状态码；有响应体时默认 200，没有时是 204。 */
  readonly status?: 200 | 201;
}

function endpoint<const E extends Endpoint>(definition: E): E {
  return definition;
}

/** 路径参数（调用方传入的值）。 */
export type ParamsOf<E extends Endpoint> = E["params"] extends z.ZodType ? z.input<E["params"]> : undefined;
/** 查询参数（调用方传入的值）。 */
export type QueryOf<E extends Endpoint> = E["query"] extends z.ZodType ? z.input<E["query"]> : undefined;
/** 请求体（调用方传入的值）。 */
export type BodyOf<E extends Endpoint> = E["body"] extends z.ZodType ? z.input<E["body"]> : undefined;
/** 响应：Server 返回的值。 */
// biome-ignore lint/suspicious/noConfusingVoidType: 没有响应体的接口，处理函数不写 return，推导出的返回类型是 void，写成 undefined 时它们无法通过类型检查。
export type ReplyOf<E extends Endpoint> = E["response"] extends z.ZodType ? z.input<E["response"]> : void;
/** 响应：客户端校验后得到的值。 */
export type ResponseOf<E extends Endpoint> = E["response"] extends z.ZodType ? z.output<E["response"]> : undefined;

/** 错误响应一律是 `{ error: 原因 }`。任务与提醒被拒绝时另带 `refusal`，`crew` 据此写英文说明。 */
export const ErrorBody = z.object({ error: z.string(), refusal: z.union([TaskRefusal, ReminderRefusal]).optional() });

/**
 * 房间里的一条消息，界面读取房间时得到。Agent 的消息带着它所在的那一轮（`runId`），
 * 以及发出前被 HELD 拦下的次数（`heldBefore`）。
 */
export const RoomMessage = MessageView.extend({
  roomId: RoomId,
  runId: z.uuid().nullable(),
  heldBefore: z.number().int().nonnegative(),
});
export type RoomMessage = z.infer<typeof RoomMessage>;

/** 写入一条消息后返回它的位置。 */
export const PostedMessage = z.object({ id: MessageId, roomId: RoomId, seq: z.number().int().positive() });

/** 界面看到的 Agent：Computer 需要的字段，加上创建时间与当前状态。 */
export const DesktopAgent = ComputerAgent.extend({ createdAt: z.string(), status: AgentStatus });
export type DesktopAgent = z.infer<typeof DesktopAgent>;

/** 新建 Agent 的输入。名字与人设的长度上限在界面的输入框里同样生效。 */
export const DISPLAY_NAME_MAX = 40;
export const PERSONA_MAX = 4_000;
export const NewAgent = z.object({
  displayName: z.string().trim().min(1, "名字不能为空").max(DISPLAY_NAME_MAX, `名字最多 ${DISPLAY_NAME_MAX} 字符`),
  persona: z
    .string()
    .trim()
    .min(1, "人设不能为空")
    .max(PERSONA_MAX, `人设最多 ${PERSONA_MAX.toLocaleString("en-US")} 字符`),
  model: z.string().min(1, "请选择模型"),
  handle: Handle,
});

/** 界面看到的群聊。私聊不在其中：它们随 Agent 列表出现。 */
export const DesktopGroup = z.object({
  id: RoomId,
  name: z.string(),
  agentIds: z.array(AgentId),
  createdAt: z.string(),
});
export type DesktopGroup = z.infer<typeof DesktopGroup>;

const AgentIds = z
  .array(AgentId)
  .min(1, "至少选择一个 agent")
  .refine((ids) => new Set(ids).size === ids.length, "agent 不能重复");

export const NewGroup = z.object({ name: RoomName, agentIds: AgentIds });

/**
 * 群聊里一条消息下的讨论串，显示在那条消息下面，也在讨论串列表里。`replies` 是讨论串里的消息数，
 * `participants` 是发过言的人（按第一次发言排列，至多 5 个），`unread` 是用户还没读的、别人发的回复数。
 */
export const THREAD_PARTICIPANTS_MAX = 5;
export const ThreadSummary = z.object({
  id: RoomId,
  /** 讨论串挂着的那条消息。 */
  parent: MessageView,
  replies: z.number().int().nonnegative(),
  lastReplyAt: z.string().nullable(),
  participants: z.array(Participant),
  unread: z.number().int().nonnegative(),
});
export type ThreadSummary = z.infer<typeof ThreadSummary>;

/** 侧栏会话列表的一项：用户所在的一个房间。讨论串不单独出现，它的未读算进所在的群聊。 */
export const Conversation = z.object({
  roomId: RoomId,
  kind: RoomKind,
  /** 群聊的名字；私聊是 Agent 的名字。 */
  name: z.string(),
  agentIds: z.array(AgentId),
  /** 最后一条消息；正文截短为预览。房间还没有消息时为 null。 */
  lastMessage: z.object({ author: Participant, body: z.string(), createdAt: z.string() }).nullable(),
  /** 别人发的、用户还没读的消息数，包括这个群聊的讨论串里的。 */
  unread: z.number().int().nonnegative(),
  /** 最后活动的时间（包括讨论串里的消息）。列表按它从新到旧排列。 */
  activeAt: z.string(),
});
export type Conversation = z.infer<typeof Conversation>;

/** 读取房间消息时的位置。查询参数在 URL 里是字符串，这里转成整数。 */
const Seq = z.union([z.number().int().nonnegative(), z.string().regex(/^\d+$/, "必须是非负整数").transform(Number)]);

/** 一次最多返回的消息数，也是不指定 `limit` 时的条数。 */
export const MESSAGE_PAGE_MAX = 200;
export const MessageWindow = z
  .object({
    /** 只取序号大于它的消息，从旧到新。 */
    after: Seq.optional(),
    /** 只取序号小于它的消息中最新的一批。 */
    before: Seq.optional(),
    limit: Seq.pipe(z.number().min(1).max(MESSAGE_PAGE_MAX)).optional(),
  })
  .refine((window) => window.after === undefined || window.before === undefined, "after 与 before 不能同时使用");

const RoomParams = z.object({ roomId: RoomId });
/** 任务编号在 URL 里是字符串，这里转成正整数。 */
const TaskNumber = z.union([
  z.number().int().positive(),
  z
    .string()
    .regex(/^[1-9]\d*$/, "任务编号必须是正整数")
    .transform(Number),
]);
const TaskParams = z.object({ roomId: RoomId, number: TaskNumber });
/** Agent 指定任务：房间（或任务的讨论串）与编号。 */
const AgentTaskRef = z.object({ roomId: RoomId, number: z.number().int().positive() });
const RunParams = z.object({ runId: z.uuid() });
const AgentParams = z.object({ agentId: AgentId });

export const api = {
  desktop: {
    listAgents: endpoint({ method: "GET", path: "/desktop/agents", response: z.array(DesktopAgent) }),
    createAgent: endpoint({
      method: "POST",
      path: "/desktop/agents",
      body: NewAgent,
      response: DesktopAgent,
      status: 201,
    }),
    /**
     * 房间的一段消息，按序号从旧到新。不带 `after` 时取最新的一批（或 `before` 之前最新的一批）。
     * 房间内序号从 1 开始连续，最早一条的序号大于 1 就说明还有更早的消息。
     */
    listMessages: endpoint({
      method: "GET",
      path: "/desktop/rooms/:roomId/messages",
      params: RoomParams,
      query: MessageWindow,
      response: z.array(RoomMessage),
    }),
    sendMessage: endpoint({
      method: "POST",
      path: "/desktop/rooms/:roomId/messages",
      params: RoomParams,
      /** 带 `threadOf` 时发到群聊里这条消息的讨论串，讨论串还没有时创建。返回的 `roomId` 是讨论串。 */
      body: z.object({ body: MessageBody, threadOf: MessageId.optional() }),
      response: PostedMessage,
      status: 201,
    }),
    /** 房间里的全部任务，按编号排列。 */
    listTasks: endpoint({
      method: "GET",
      path: "/desktop/rooms/:roomId/tasks",
      params: RoomParams,
      response: z.array(TaskView),
    }),
    /** 新建任务：以用户的身份发一条正文为标题的消息，再把它变成任务。 */
    createTask: endpoint({
      method: "POST",
      path: "/desktop/rooms/:roomId/tasks",
      params: RoomParams,
      body: z.object({ title: TaskTitle, assigneeId: AgentId.optional() }),
      response: TaskView,
      status: 201,
    }),
    convertToTask: endpoint({
      method: "POST",
      path: "/desktop/rooms/:roomId/tasks/convert",
      params: RoomParams,
      body: z.object({ messageId: MessageId, assigneeId: AgentId.optional() }),
      response: TaskView,
      status: 201,
    }),
    setTaskStatus: endpoint({
      method: "POST",
      path: "/desktop/rooms/:roomId/tasks/:number/status",
      params: TaskParams,
      body: z.object({ status: TaskStatus }),
      response: TaskView,
    }),
    /** 换负责人；`agentId` 为 null 时取消负责人。 */
    assignTask: endpoint({
      method: "POST",
      path: "/desktop/rooms/:roomId/tasks/:number/assignee",
      params: TaskParams,
      body: z.object({ agentId: AgentId.nullable() }),
      response: TaskView,
    }),
    /** 群聊里的全部讨论串。 */
    listThreads: endpoint({
      method: "GET",
      path: "/desktop/rooms/:roomId/threads",
      params: RoomParams,
      response: z.array(ThreadSummary),
    }),
    listModels: endpoint({ method: "GET", path: "/desktop/models", response: z.array(z.string()) }),
    listConversations: endpoint({
      method: "GET",
      path: "/desktop/conversations",
      response: z.array(Conversation),
    }),
    /** 用户读到了这个房间的第 `seq` 条。只前进。 */
    markRead: endpoint({
      method: "POST",
      path: "/desktop/rooms/:roomId/read",
      params: RoomParams,
      body: z.object({ seq: z.number().int().nonnegative() }),
    }),
    listGroups: endpoint({ method: "GET", path: "/desktop/groups", response: z.array(DesktopGroup) }),
    /**
     * 运行记录，从新到旧。给 `roomId` 时只列这个房间（包括它的讨论串）唤醒的轮次，给 `agentId` 时只列这个 Agent 的。
     */
    listRuns: endpoint({
      method: "GET",
      path: "/desktop/runs",
      query: z.object({
        roomId: RoomId.optional(),
        agentId: AgentId.optional(),
      }),
      response: z.array(RunSummary),
    }),
    getRun: endpoint({ method: "GET", path: "/desktop/runs/:runId", params: RunParams, response: RunDetail }),
    createGroup: endpoint({
      method: "POST",
      path: "/desktop/groups",
      body: NewGroup,
      response: DesktopGroup,
      status: 201,
    }),
    /** 把 Agent 加进群聊。已经在群里的 Agent 不变。 */
    addGroupMembers: endpoint({
      method: "POST",
      path: "/desktop/groups/:roomId/members",
      params: RoomParams,
      body: z.object({ agentIds: AgentIds }),
      response: DesktopGroup,
    }),
  },
  computer: {
    /** Computer 启动时调用：确认地址与凭证可用，并把上一个 Computer 没结束的轮次标为中断。 */
    connect: endpoint({ method: "POST", path: "/computer/connect" }),
    listAgents: endpoint({ method: "GET", path: "/computer/agents", response: z.array(ComputerAgent) }),
    /**
     * 取出 Agent 已读位置之后的消息，同时把它们记为已投递：之后 Agent 回复时，
     * 只有更新的消息会让回复被拦下（HELD）。所以是 POST。
     */
    readInbox: endpoint({
      method: "POST",
      path: "/computer/agents/:agentId/inbox",
      params: AgentParams,
      response: z.array(InboxRoom),
    }),
    /** Turn 成功后调用：每个房间的已读位置推进到已投递位置。 */
    acknowledge: endpoint({
      method: "POST",
      path: "/computer/agents/:agentId/inbox/ack",
      params: AgentParams,
    }),
    issueAgentToken: endpoint({
      method: "POST",
      path: "/computer/agents/:agentId/token",
      params: AgentParams,
      response: z.object({ token: z.string().min(1) }),
    }),
    /**
     * Agent 跑不起来的原因（沙箱不可用、目录不安全等），与某一轮无关。`problem` 为 null 时清除。
     * 一轮里的失败记在运行记录里，不经过这里。
     */
    reportProblem: endpoint({
      method: "POST",
      path: "/computer/agents/:agentId/problem",
      params: AgentParams,
      body: z.object({ problem: z.string().min(1).nullable() }),
    }),
    /** 开始一轮：登记被哪些消息唤醒与这一轮的完整输入，返回 run ID。 */
    startRun: endpoint({
      method: "POST",
      path: "/computer/agents/:agentId/runs",
      params: AgentParams,
      body: z.object({ prompt: z.string(), triggers: z.array(RunTrigger).min(1) }),
      response: z.object({ id: z.uuid() }),
      status: 201,
    }),
    /** 按发生顺序追加 Engine 事件。 */
    appendRunEvents: endpoint({
      method: "POST",
      path: "/computer/runs/:runId/events",
      params: RunParams,
      body: z.object({ events: z.array(EngineEvent).min(1) }),
    }),
    /** 一轮结束。失败时带原因。 */
    finishRun: endpoint({
      method: "POST",
      path: "/computer/runs/:runId/finish",
      params: RunParams,
      body: z.discriminatedUnion("outcome", [
        z.object({ outcome: z.literal("succeeded") }),
        z.object({ outcome: z.literal("cancelled") }),
        z.object({ outcome: z.literal("failed"), error: z.string().min(1) }),
      ]),
    }),
    reportModels: endpoint({
      method: "POST",
      path: "/computer/models",
      body: z.object({ models: z.array(z.string().min(1)) }),
    }),
  },
  agent: {
    /** 给自己定一个提醒。 */
    createReminder: endpoint({
      method: "POST",
      path: "/agent/reminders/create",
      body: NewReminder,
      response: ReminderView,
    }),
    /** 自己还没触发的提醒，按触发时间排列。 */
    listReminders: endpoint({ method: "POST", path: "/agent/reminders/list", response: z.array(ReminderView) }),
    cancelReminder: endpoint({
      method: "POST",
      path: "/agent/reminders/cancel",
      body: z.object({ id: z.uuid() }),
      response: ReminderView,
    }),
    listTasks: endpoint({
      method: "POST",
      path: "/agent/tasks/list",
      body: z.object({ roomId: RoomId }),
      response: z.array(TaskView),
    }),
    createTask: endpoint({
      method: "POST",
      path: "/agent/tasks/create",
      body: z.object({ roomId: RoomId, title: TaskTitle, assign: Handle.optional() }),
      response: TaskView,
    }),
    convertToTask: endpoint({
      method: "POST",
      path: "/agent/tasks/convert",
      body: z.object({ roomId: RoomId, messageId: MessageId, assign: Handle.optional() }),
      response: TaskView,
    }),
    claimTask: endpoint({ method: "POST", path: "/agent/tasks/claim", body: AgentTaskRef, response: TaskView }),
    setTaskStatus: endpoint({
      method: "POST",
      path: "/agent/tasks/status",
      body: AgentTaskRef.extend({ status: TaskStatus }),
      response: TaskView,
    }),
    assignTask: endpoint({
      method: "POST",
      path: "/agent/tasks/assign",
      body: AgentTaskRef.extend({ assign: Handle }),
      response: TaskView,
    }),
    reply: endpoint({
      method: "POST",
      path: "/agent/reply",
      /** 带 `threadOf` 时发到 `roomId` 里这条消息的讨论串，讨论串还没有时创建。 */
      body: z.object({ roomId: RoomId, body: MessageBody, threadOf: MessageId.optional() }),
      response: ReplyOutcome,
    }),
  },
} as const;

/** SSE 接口不在上面的契约里：它们不返回一个 JSON 响应，而是一条事件流。 */
export const EVENT_STREAMS = { desktop: "/desktop/events", computer: "/computer/events" } as const;

/** 把 `:name` 换成参数值。 */
export function endpointPath(path: string, params: Record<string, string> | undefined): string {
  return path.replace(/:([A-Za-z]+)/g, (_match, name: string) => {
    const value = params?.[name];
    if (value === undefined) throw new Error(`缺少路径参数 ${name}`);
    return encodeURIComponent(value);
  });
}
