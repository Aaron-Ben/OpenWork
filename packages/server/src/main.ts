import type { Server } from "node:http";
import { createInterface } from "node:readline";
import { encodeMessage, readMessage, ServerBootstrap, type ServerReady } from "@crew/protocol";
import { serve } from "@hono/node-server";
import { Redis } from "ioredis";
import pg from "pg";
import { createApp } from "./app";
import { createDatabase, ensureLocalUser, migrateDatabase } from "./db";

// Server 进程入口，由 Desktop 主进程启动。stdout 只用来写 ready 消息，日志一律写 stderr。

/** 关闭时等待连接自然结束的时间，超时后强制断开（例如长连接的 SSE）。 */
const SHUTDOWN_GRACE_MS = 3_000;

function requireEnv(name: string): string {
  const value = process.env[name];
  if (!value) {
    throw new Error(`缺少环境变量 ${name}`);
  }
  return value;
}

async function connectPostgres(connectionString: string): Promise<pg.Pool> {
  const pool = new pg.Pool({ connectionString });
  await pool.query("SELECT 1");
  return pool;
}

async function connectRedis(url: string): Promise<Redis> {
  // 第 1 步只在启动时检查 Redis，连不上就让启动失败，所以关闭自动重连。
  const redis = new Redis(url, { lazyConnect: true, retryStrategy: () => null });
  redis.on("error", (error) => console.error("[server] Redis 错误:", error.message));
  await redis.connect();
  await redis.ping();
  return redis;
}

function listen(app: ReturnType<typeof createApp>): Promise<{ server: Server; port: number }> {
  return new Promise((resolve) => {
    const server = serve({ fetch: app.fetch, hostname: "127.0.0.1", port: 0 }, (info) => {
      resolve({ server: server as Server, port: info.port });
    });
  });
}

async function closeServer(server: Server): Promise<void> {
  const closed = new Promise<void>((resolve) => server.close(() => resolve()));
  const forceClose = setTimeout(() => server.closeAllConnections(), SHUTDOWN_GRACE_MS);
  await closed;
  clearTimeout(forceClose);
}

async function main(): Promise<void> {
  const databaseUrl = requireEnv("DATABASE_URL");
  const redisUrl = requireEnv("REDIS_URL");
  const rendererOrigin = requireEnv("CREW_RENDERER_ORIGIN");
  const migrationsDir = requireEnv("CREW_MIGRATIONS_DIR");

  const bootstrap = await readMessage(createInterface({ input: process.stdin }), ServerBootstrap);

  const pool = await connectPostgres(databaseUrl);
  const db = createDatabase(pool);
  await migrateDatabase(db, migrationsDir);
  await ensureLocalUser(db);
  const redis = await connectRedis(redisUrl);

  const app = createApp({
    desktopToken: bootstrap.desktopToken,
    computerToken: bootstrap.computerToken,
    rendererOrigin,
  });
  const { server, port } = await listen(app);

  const ready: ServerReady = {
    runtimeSessionId: bootstrap.runtimeSessionId,
    baseUrl: `http://127.0.0.1:${port}`,
  };
  process.stdout.write(encodeMessage(ready));

  let shuttingDown = false;
  const shutdown = async () => {
    if (shuttingDown) return;
    shuttingDown = true;
    await closeServer(server);
    await pool.end();
    await redis.quit();
    process.exit(0);
  };
  process.once("SIGTERM", shutdown);
  process.once("SIGINT", shutdown);
  // 父进程退出（包括被 SIGKILL）时 stdin 关闭，Server 随之关闭，不留下孤儿进程。
  process.stdin.once("end", shutdown);
  process.stdin.resume();
}

main().catch((error: unknown) => {
  console.error("[server] 启动失败:", error);
  process.exit(1);
});
