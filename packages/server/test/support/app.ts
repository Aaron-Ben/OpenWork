import type { Server } from "node:http";
import { createApp } from "../../src/app";
import type { ServerContext } from "../../src/context";
import { ensureLocalUser } from "../../src/db";
import { EventHub } from "../../src/events";
import { ReminderScheduler } from "../../src/reminders";
import { closeServer, listen } from "../../src/serve";
import { RuntimeState } from "../../src/state";
import { createTestDatabase, type TestDatabase } from "./database";

export const TEST_DESKTOP_TOKEN = "desktop-token-0123456789";
export const TEST_COMPUTER_TOKEN = "computer-token-0123456789";
export const TEST_RENDERER_ORIGIN = "http://localhost:5173";

export interface TestApp {
  ctx: ServerContext;
  server: Server;
  /** Server 实际监听的地址，例如 `http://127.0.0.1:52341`。 */
  baseUrl: string;
  /** 向 Server 发一个请求；`path` 是相对 Server 根目录的路径。 */
  request(path: string, init?: RequestInit): Promise<Response>;
  /**
   * 可作为 `fetch` 传给客户端：不论 URL 写的是哪个主机，都发到这个 Server。
   * 客户端可以用一个固定的假地址（例如 `http://127.0.0.1:1`），测试断言里的地址因此不随端口变化。
   */
  fetch: typeof fetch;
  /** 提醒的计时器。测试不让它自己计时，调用 `fireDue` 触发到期的提醒。 */
  reminders: ReminderScheduler;
  /** 把 Server 的“现在”定在某个时间；传 undefined 恢复真实时间。 */
  setNow(date: Date | undefined): void;
  /** 结束 SSE、关闭 Server，并删除临时数据库。 */
  close(): Promise<void>;
}

/**
 * 在一个临时数据库上组装完整的 Server 应用，监听在 `127.0.0.1` 的随机端口上。
 * 请求经过真实的 HTTP 连接，与生产环境一致。
 */
export async function createTestApp(): Promise<TestApp> {
  const database: TestDatabase = await createTestDatabase();
  let fixedNow: Date | undefined;
  const now = () => fixedNow ?? new Date();
  const events = new EventHub();
  const reminders = new ReminderScheduler({ db: database.db, events, now });
  const ctx: ServerContext = {
    db: database.db,
    localUserId: await ensureLocalUser(database.db),
    state: new RuntimeState(),
    events,
    now,
    reminders,
  };
  const app = createApp({
    ...ctx,
    desktopToken: TEST_DESKTOP_TOKEN,
    computerToken: TEST_COMPUTER_TOKEN,
    rendererOrigin: TEST_RENDERER_ORIGIN,
  });
  const { server, port } = await listen(app);
  const baseUrl = `http://127.0.0.1:${port}`;

  /** 保留路径与查询，换成这个 Server 的地址。 */
  const toServer = (url: string | URL) => {
    const parsed = new URL(url);
    return new URL(`${parsed.pathname}${parsed.search}`, baseUrl);
  };

  return {
    ctx,
    server,
    baseUrl,
    reminders,
    setNow: (date) => {
      fixedNow = date;
    },
    request: (path, init) => fetch(new URL(path, baseUrl), init),
    fetch: async (input, init) => {
      if (input instanceof Request) return fetch(new Request(toServer(input.url), input), init);
      return fetch(toServer(input), init);
    },
    close: async () => {
      ctx.events.close();
      await closeServer(server);
      await database.drop();
    },
  };
}
