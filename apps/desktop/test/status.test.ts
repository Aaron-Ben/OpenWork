import { createApp } from "@crew/server";
import { describe, expect, it } from "vitest";
import { createServerClient, loadStatus } from "../src/renderer/status";

// 用内存中的真实 Server 应用测试渲染进程的客户端：凭证随请求发送，状态与失败都转成页面能显示的结果。

const desktopToken = "desktop-token-0123456789";
const computerToken = "computer-token-0123456789";

function inMemoryServer() {
  const app = createApp({ desktopToken, computerToken, rendererOrigin: "http://localhost:5173" });
  const fetchFn: typeof fetch = async (input, init) => app.request(input, init);
  return { app, fetchFn };
}

describe("loadStatus", () => {
  it("reports that the computer has not connected yet", async () => {
    const { fetchFn } = inMemoryServer();
    const client = createServerClient("http://127.0.0.1:1", desktopToken, fetchFn);
    expect(await loadStatus(client)).toEqual({ kind: "connected", computerConnected: false });
  });

  it("reports a connected computer", async () => {
    const { app, fetchFn } = inMemoryServer();
    await app.request("/computer/connect", { method: "POST", headers: { Authorization: `Bearer ${computerToken}` } });
    const client = createServerClient("http://127.0.0.1:1", desktopToken, fetchFn);
    expect(await loadStatus(client)).toEqual({ kind: "connected", computerConnected: true });
  });

  it("turns a rejected token into a failure with the status code", async () => {
    const { fetchFn } = inMemoryServer();
    const client = createServerClient("http://127.0.0.1:1", "wrong-token", fetchFn);
    expect(await loadStatus(client)).toEqual({ kind: "failed", reason: "Server 返回 401" });
  });

  it("turns a network error into a failure", async () => {
    const client = createServerClient("http://127.0.0.1:1", desktopToken);
    const status = await loadStatus(client);
    expect(status.kind).toBe("failed");
  });
});
