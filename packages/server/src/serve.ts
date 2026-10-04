import type { Server } from "node:http";
import { serve } from "@hono/node-server";
import type { createApp } from "./app";

// HTTP 服务的启动与关闭。从 main.ts 分出来，测试能用真实的 HTTP 连接验证关闭行为。

/** 关闭时等正在进行的请求结束的最长时间，超时后强制断开。必须小于主进程给 Server 的宽限期。 */
const SHUTDOWN_GRACE_MS = 2_000;

/** 在 `127.0.0.1` 的随机端口上监听。 */
export function listen(app: ReturnType<typeof createApp>): Promise<{ server: Server; port: number }> {
  return new Promise((resolve) => {
    const server = serve({ fetch: app.fetch, hostname: "127.0.0.1", port: 0 }, (info) => {
      resolve({ server: server as Server, port: info.port });
    });
  });
}

/**
 * 停止接受新连接，等正在进行的请求结束后关闭。调用前先关闭事件通道（`EventHub.close`），
 * SSE 长连接才会结束；否则这里要等满宽限期。
 */
export async function closeServer(server: Server): Promise<void> {
  const closed = new Promise<void>((resolve) => server.close(() => resolve()));
  const forceClose = setTimeout(() => server.closeAllConnections(), SHUTDOWN_GRACE_MS);
  await closed;
  clearTimeout(forceClose);
}
