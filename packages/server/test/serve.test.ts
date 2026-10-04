import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { closeServer } from "../src/serve";
import { createTestApp, TEST_DESKTOP_TOKEN, type TestApp } from "./support/app";

// 用真实的 HTTP 连接测试关闭：内存中的请求没有连接复用，测不出 keep-alive 连接拖住 server.close() 的问题。

let t: TestApp;
beforeEach(async () => {
  t = await createTestApp();
});
afterEach(async () => {
  await t.close();
});

describe("closeServer", () => {
  it("closes promptly while the desktop is reading the event stream, once the channels are closed", async () => {
    const controller = new AbortController();
    const response = await t.request("/desktop/events", {
      headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}` },
      signal: controller.signal,
    });
    expect(response.headers.get("Connection")).toBe("close");
    const reader = response.body?.getReader();
    const reading = (async () => {
      while (reader && !(await reader.read()).done) {
        // 和界面一样持续读取。
      }
    })().catch(() => undefined);

    const started = Date.now();
    t.ctx.events.close();
    await closeServer(t.server);
    expect(Date.now() - started).toBeLessThan(1_000);

    controller.abort();
    await reading;
  });
});
