import { createTestApp, TEST_DESKTOP_TOKEN, type TestApp } from "@crew/server/testing";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createServerClient, request } from "../src/renderer/lib/server";

// 用内存中的真实 Server 应用测试界面的客户端：凭证随请求发送，失败时抛出 Server 给出的原因。

let t: TestApp;
beforeEach(async () => {
  t = await createTestApp();
});
afterEach(async () => {
  await t.close();
});

const client = (token = TEST_DESKTOP_TOKEN) => createServerClient("http://127.0.0.1:1", token, t.fetch);

describe("request", () => {
  it("returns the typed data of a successful response", async () => {
    const agent = await request(
      client().desktop.agents.$post({ json: { displayName: "Alice", persona: "代码审查者", model: "a/b" } }),
    );
    expect(agent).toMatchObject({ displayName: "Alice", status: { state: "idle" } });
    expect(await request(client().desktop.agents.$get())).toHaveLength(1);
  });

  it("throws the server's reason for a rejected request", async () => {
    await expect(
      request(client().desktop.agents.$post({ json: { displayName: " ", persona: "x", model: "a/b" } })),
    ).rejects.toThrow("名字不能为空");
  });

  it("throws the server's reason for a rejected token", async () => {
    await expect(request(client("wrong").desktop.agents.$get())).rejects.toThrow("凭证无效");
  });
});
