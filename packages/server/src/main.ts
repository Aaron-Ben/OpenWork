import { createInterface } from "node:readline";
import { encodeMessage, readMessage, ServerBootstrap, type ServerReady } from "@crew/protocol";
import pg from "pg";
import { createApp } from "./app";
import { createDatabase, ensureLocalUser, migrateDatabase } from "./db";
import { EventHub } from "./events";
import { closeServer, listen } from "./serve";
import { RuntimeState } from "./state";

// Server 进程入口，由 Desktop 主进程启动。stdout 只用来写 ready 消息，日志一律写 stderr。

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

async function main(): Promise<void> {
  const databaseUrl = requireEnv("DATABASE_URL");
  const rendererOrigin = requireEnv("CREW_RENDERER_ORIGIN");
  const migrationsDir = requireEnv("CREW_MIGRATIONS_DIR");

  const bootstrap = await readMessage(createInterface({ input: process.stdin }), ServerBootstrap);

  const pool = await connectPostgres(databaseUrl);
  const db = createDatabase(pool);
  await migrateDatabase(db, migrationsDir);
  const localUserId = await ensureLocalUser(db);

  const events = new EventHub();
  const app = createApp({
    desktopToken: bootstrap.desktopToken,
    computerToken: bootstrap.computerToken,
    rendererOrigin,
    db,
    localUserId,
    state: new RuntimeState(),
    events,
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
    // 先结束 SSE 长连接，server.close() 才不用等满宽限期；否则主进程会先发 SIGKILL。
    events.close();
    await closeServer(server);
    await pool.end();
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
