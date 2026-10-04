import { ApiClient, ApiError, api } from "@crew/protocol";
import { createTestApp, TEST_DESKTOP_TOKEN, type TestApp } from "@crew/server/testing";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// 界面用 protocol 的 ApiClient 按契约调用真实的 Server：凭证随请求发送，失败时抛出 Server 给出的原因。

let t: TestApp;
beforeEach(async () => {
  t = await createTestApp();
});
afterEach(async () => {
  await t.close();
});

const client = (token = TEST_DESKTOP_TOKEN) => new ApiClient({ baseUrl: t.baseUrl, token });

describe("ApiClient against the server", () => {
  it("returns the validated data of a successful response", async () => {
    const agent = await client().call(api.desktop.createAgent, {
      body: { displayName: "Alice", handle: "alice", persona: "代码审查者", model: "a/b" },
    });
    expect(agent).toMatchObject({ displayName: "Alice", status: { state: "idle" } });
    expect(await client().call(api.desktop.listAgents)).toHaveLength(1);
  });

  it("throws the server's reason for a rejected request", async () => {
    await expect(
      client().call(api.desktop.createAgent, {
        body: { displayName: " ", handle: "blank", persona: "x", model: "a/b" },
      }),
    ).rejects.toThrow("名字不能为空");
  });

  it("throws the server's reason and status for a rejected token", async () => {
    const error = await client("wrong")
      .call(api.desktop.listAgents)
      .catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({ status: 401, message: "凭证无效" });
  });
});
