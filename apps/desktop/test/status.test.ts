import { createTestApp, TEST_COMPUTER_TOKEN, TEST_DESKTOP_TOKEN, type TestApp } from "@crew/server/testing";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createServerClient, loadStatus } from "../src/renderer/status";

// 用内存中的真实 Server 应用测试渲染进程的客户端：凭证随请求发送，状态与失败都转成页面能显示的结果。

let t: TestApp;
beforeEach(async () => {
  t = await createTestApp();
});
afterEach(async () => {
  await t.close();
});

describe("loadStatus", () => {
  it("reports that the computer has not connected yet", async () => {
    const client = createServerClient("http://127.0.0.1:1", TEST_DESKTOP_TOKEN, t.fetch);
    expect(await loadStatus(client)).toEqual({ kind: "connected", computerConnected: false });
  });

  it("reports a connected computer", async () => {
    await t.app.request("/computer/connect", {
      method: "POST",
      headers: { Authorization: `Bearer ${TEST_COMPUTER_TOKEN}` },
    });
    const client = createServerClient("http://127.0.0.1:1", TEST_DESKTOP_TOKEN, t.fetch);
    expect(await loadStatus(client)).toEqual({ kind: "connected", computerConnected: true });
  });

  it("turns a rejected token into a failure with the status code", async () => {
    const client = createServerClient("http://127.0.0.1:1", "wrong-token", t.fetch);
    expect(await loadStatus(client)).toEqual({ kind: "failed", reason: "Server 返回 401" });
  });

  it("turns a network error into a failure", async () => {
    const client = createServerClient("http://127.0.0.1:1", TEST_DESKTOP_TOKEN);
    const status = await loadStatus(client);
    expect(status.kind).toBe("failed");
  });
});
