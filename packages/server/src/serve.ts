import { createServer, type Server } from "node:http";
import type { Express } from "express";

// HTTP 服务的启动与关闭。从 main.ts 分出来，测试能用真实的 HTTP 连接验证关闭行为。

/** 关闭时等正在进行的请求结束的最长时间，超时后强制断开。必须小于主进程给 Server 的宽限期。 */
const SHUTDOWN_GRACE_MS = 2_000;

/** 在 `127.0.0.1` 的随机端口上监听。 */
export function listen(app: Express): Promise<{ server: Server; port: number }> {
  const server = createServer(app);
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (address === null || typeof address === "string") {
        reject(new Error("Server 没有监听在 TCP 端口上"));
        return;
      }
      resolve({ server, port: address.port });
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
