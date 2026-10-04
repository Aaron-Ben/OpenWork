import type { Context, MiddlewareHandler } from "hono";
import { bearerAuth } from "hono/bearer-auth";
import { streamSSE } from "hono/streaming";
import { validator } from "hono/validator";
import type { z } from "zod";
import type { Channel } from "./events";

// 路由共用的小工具：凭证校验、请求校验与 SSE。错误响应一律是 `{ error: 原因 }`。

/** 只接受给定凭证的 bearer 认证。错误响应与其他接口一致。 */
export function requireToken(token: string): MiddlewareHandler {
  const message = { error: "凭证无效" };
  return bearerAuth({
    token,
    noAuthenticationHeader: { message },
    invalidAuthenticationHeader: { message },
    invalidToken: { message },
  });
}

/** 用 zod 校验请求的一部分。不合法时返回 400 与第一条错误。`hono/client` 由此得知请求的类型。 */
export function validate<Target extends "json" | "param", Schema extends z.ZodType>(target: Target, schema: Schema) {
  return validator(target, (value, c) => {
    const result = schema.safeParse(value);
    if (!result.success) {
      return c.json({ error: result.error.issues[0]?.message ?? "请求参数不合法" }, 400);
    }
    return result.data;
  });
}

/** SSE 连接空闲时发送注释行的间隔，避免中间层把连接当作空闲关闭。 */
export const SSE_KEEPALIVE_MS = 15_000;

/** 把一个事件通道转成 SSE 响应。连接断开或通道关闭时结束，并取消订阅。 */
export function eventStream<T>(c: Context, channel: Channel<T>): Response {
  const response = streamSSE(c, async (stream) => {
    const unsubscribe = channel.subscribe((event) => {
      void stream.writeSSE({ data: JSON.stringify(event) });
    });
    const keepalive = setInterval(() => {
      void stream.write(": keepalive\n\n");
    }, SSE_KEEPALIVE_MS);
    // 客户端断开，或 Server 关闭通道：两者都结束这个响应。
    await Promise.race([new Promise<void>((resolve) => stream.onAbort(resolve)), channel.closed]);
    clearInterval(keepalive);
    unsubscribe();
  });
  // 响应结束时同时关闭连接：否则 SSE 结束后连接空闲地留着，Server 关闭时 server.close() 要等它的 keep-alive
  // 超时（5 秒），超过主进程给的宽限，被 SIGKILL。streamSSE 用 c.header() 写入 keep-alive，Hono 设置响应时
  // 会把上下文里的响应头复制上去，所以要在它之后用 c.header() 改掉。
  c.header("Connection", "close");
  return response;
}
