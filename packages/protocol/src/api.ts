import { z } from "zod";
import {
  AgentStatus,
  ComputerAgent,
  Handle,
  InboxRoom,
  MessageBody,
  MessageView,
  ReplyOutcome,
  RoomName,
} from "./collab";
import { AgentId, MessageId, RoomId } from "./ids";

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

/** 错误响应一律是 `{ error: 原因 }`。 */
export const ErrorBody = z.object({ error: z.string() });

/** 房间里的一条消息，界面读取房间时得到。 */
export const RoomMessage = MessageView.extend({ roomId: RoomId });
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
      body: z.object({ body: MessageBody }),
      response: PostedMessage,
      status: 201,
    }),
    listModels: endpoint({ method: "GET", path: "/desktop/models", response: z.array(z.string()) }),
    listGroups: endpoint({ method: "GET", path: "/desktop/groups", response: z.array(DesktopGroup) }),
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
    reportStatus: endpoint({
      method: "POST",
      path: "/computer/agents/:agentId/status",
      params: AgentParams,
      body: AgentStatus,
    }),
    reportModels: endpoint({
      method: "POST",
      path: "/computer/models",
      body: z.object({ models: z.array(z.string().min(1)) }),
    }),
  },
  agent: {
    reply: endpoint({
      method: "POST",
      path: "/agent/reply",
      body: z.object({ roomId: RoomId, body: MessageBody }),
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
