import { timingSafeEqual } from "node:crypto";
import { AgentId, type Endpoint, type ReplyOf } from "@crew/protocol";
import type { ErrorRequestHandler, Express, Request, RequestHandler, Response } from "express";
import { z } from "zod";
import type { ServerContext } from "./context";
import { RequestError } from "./errors";
import type { Channel } from "./events";

// 路由共用的小工具：凭证校验、按契约注册接口、SSE 与错误处理。错误响应一律是 `{ error: 原因 }`。

function bearerToken(request: Request): string | undefined {
  return request.get("Authorization")?.match(/^Bearer (.+)$/)?.[1];
}

const INVALID_TOKEN = { error: "凭证无效" };

/** 只接受给定凭证的 bearer 认证。比较用固定时长，不因凭证前缀相同而更快返回。 */
export function requireToken(token: string): RequestHandler {
  const expected = Buffer.from(token);
  return (request, response, next) => {
    const given = Buffer.from(bearerToken(request) ?? "");
    if (given.length === expected.length && timingSafeEqual(given, expected)) {
      next();
      return;
    }
    response.status(401).json(INVALID_TOKEN);
  };
}

/** Agent 凭证：凭证决定是哪个 Agent，请求体里不能指定身份。 */
export function requireAgent(ctx: ServerContext): RequestHandler {
  return (request, response, next) => {
    const token = bearerToken(request);
    const agentId = token ? ctx.state.agentForToken(token) : undefined;
    if (!agentId) {
      response.status(401).json(INVALID_TOKEN);
      return;
    }
    response.locals.agentId = agentId;
    next();
  };
}

/** `requireAgent` 认出的 Agent。 */
export function agentOf(response: Response): AgentId {
  return AgentId.parse(response.locals.agentId);
}

type Parsed<S> = S extends z.ZodType ? z.output<S> : undefined;

export interface HandlerInput<E extends Endpoint> {
  params: Parsed<E["params"]>;
  body: Parsed<E["body"]>;
  response: Response;
}

interface LooseInput {
  params: unknown;
  body: unknown;
  response: Response;
}

function parseInput(schema: z.ZodType | undefined, value: unknown): unknown {
  if (!schema) return undefined;
  const result = schema.safeParse(value);
  if (!result.success) throw new RequestError(400, result.error.issues[0]?.message ?? "请求参数不合法");
  return result.data;
}

/**
 * 按契约注册一个接口：校验路径参数与请求体，不合法时返回 400 与第一条错误；
 * 处理函数的返回值必须符合响应 schema 的类型。处理函数抛出的错误交给 `errorHandler`。
 */
export function route<E extends Endpoint>(
  app: Express,
  endpoint: E,
  handler: (input: HandlerInput<E>) => ReplyOf<E> | Promise<ReplyOf<E>>,
): void;
export function route(app: Express, endpoint: Endpoint, handler: (input: LooseInput) => unknown): void {
  const handle: RequestHandler = async (request, response) => {
    const params = parseInput(endpoint.params, request.params);
    const body = parseInput(endpoint.body, request.body);
    const reply = await handler({ params, body, response });
    if (endpoint.response) response.status(endpoint.status ?? 200).json(reply);
    else response.status(204).end();
  };
  if (endpoint.method === "GET") app.get(endpoint.path, handle);
  else app.post(endpoint.path, handle);
}

/** SSE 连接空闲时发送注释行的间隔，避免中间层把连接当作空闲关闭。 */
export const SSE_KEEPALIVE_MS = 15_000;

/**
 * 把一个事件通道写成 SSE 响应。客户端断开或通道关闭时结束，并取消订阅。
 *
 * 响应头带 `Connection: close`：流结束时连接一起关闭。否则连接以 keep-alive 的形式空闲地留着，
 * Server 关闭时 `server.close()` 要等它超时（5 秒），超过主进程给的宽限，Server 被 SIGKILL。
 */
export function eventStream<T>(request: Request, response: Response, channel: Channel<T>): void {
  response.writeHead(200, {
    "Content-Type": "text/event-stream",
    "Cache-Control": "no-cache",
    Connection: "close",
  });
  response.flushHeaders();

  const unsubscribe = channel.subscribe((event) => {
    response.write(`data: ${JSON.stringify(event)}\n\n`);
  });
  const keepalive = setInterval(() => response.write(": keepalive\n\n"), SSE_KEEPALIVE_MS);

  let ended = false;
  const end = () => {
    if (ended) return;
    ended = true;
    clearInterval(keepalive);
    unsubscribe();
    response.end();
  };
  // 客户端断开，或 Server 关闭通道：两者都结束这个响应。
  request.on("close", end);
  void channel.closed.then(end);
}

const BodyParserError = z.object({ type: z.string(), status: z.number() });

/** 把抛出的错误转成 `{ error }` 响应。未预料的错误记日志，返回 500。 */
export const errorHandler: ErrorRequestHandler = (error, _request, response, _next) => {
  if (error instanceof RequestError) {
    response.status(error.status).json({ error: error.message });
    return;
  }
  const parserError = BodyParserError.safeParse(error);
  if (parserError.success && parserError.data.type === "entity.parse.failed") {
    response.status(400).json({ error: "请求体不是合法的 JSON" });
    return;
  }
  if (parserError.success && parserError.data.type === "entity.too.large") {
    response.status(413).json({ error: "请求体过大" });
    return;
  }
  console.error("[server] 请求处理失败:", error);
  response.status(500).json({ error: "Server 内部错误" });
};

/** 没有匹配的接口。 */
export const notFound: RequestHandler = (_request, response) => {
  response.status(404).json({ error: "接口不存在" });
};
