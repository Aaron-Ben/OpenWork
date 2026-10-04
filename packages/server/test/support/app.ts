import { createApp } from "../../src/app";
import type { ServerContext } from "../../src/context";
import { ensureLocalUser } from "../../src/db";
import { EventHub } from "../../src/events";
import { RuntimeState } from "../../src/state";
import { createTestDatabase, type TestDatabase } from "./database";

export const TEST_DESKTOP_TOKEN = "desktop-token-0123456789";
export const TEST_COMPUTER_TOKEN = "computer-token-0123456789";
export const TEST_RENDERER_ORIGIN = "http://localhost:5173";

export interface TestApp {
  app: ReturnType<typeof createApp>;
  ctx: ServerContext;
  /** 把请求交给内存中的应用，可作为 `fetch` 传给客户端。 */
  fetch: typeof fetch;
  /** 关闭连接并删除临时数据库。 */
  close(): Promise<void>;
}

/** 在一个临时数据库上组装完整的 Server 应用，不监听端口。 */
export async function createTestApp(): Promise<TestApp> {
  const database: TestDatabase = await createTestDatabase();
  const ctx: ServerContext = {
    db: database.db,
    localUserId: await ensureLocalUser(database.db),
    state: new RuntimeState(),
    events: new EventHub(),
  };
  const app = createApp({
    ...ctx,
    desktopToken: TEST_DESKTOP_TOKEN,
    computerToken: TEST_COMPUTER_TOKEN,
    rendererOrigin: TEST_RENDERER_ORIGIN,
  });
  return {
    app,
    ctx,
    fetch: async (input, init) => app.request(input, init),
    close: database.drop,
  };
}
