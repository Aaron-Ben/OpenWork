import { createTestApp, TEST_COMPUTER_TOKEN, TEST_DESKTOP_TOKEN, type TestApp } from "@crew/server/testing";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { connectToServer } from "../src/connect";

const baseUrl = "http://127.0.0.1:1";

let t: TestApp;
beforeEach(async () => {
  t = await createTestApp();
});
afterEach(async () => {
  await t.close();
});

describe("connectToServer", () => {
  it("marks the computer as connected on the server", async () => {
    await connectToServer(baseUrl, TEST_COMPUTER_TOKEN, t.fetch);

    const status = await t.app.request("/desktop/status", {
      headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}` },
    });
    expect(await status.json()).toEqual({ computerConnected: true });
  });

  it("reports a rejected token", async () => {
    await expect(connectToServer(baseUrl, "wrong-token", t.fetch)).rejects.toThrow(
      "Server 拒绝了 Computer 凭证（401）",
    );
  });

  it("reports an unreachable server", async () => {
    await expect(connectToServer(baseUrl, TEST_COMPUTER_TOKEN)).rejects.toThrow(
      "无法连接 Server（http://127.0.0.1:1）",
    );
  });
});
