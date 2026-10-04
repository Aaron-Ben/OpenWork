import { createApp } from "@crew/server";
import { describe, expect, it } from "vitest";
import { connectToServer } from "../src/connect";

const computerToken = "computer-token-0123456789";
const baseUrl = "http://127.0.0.1:1";

function inMemoryServer() {
  const app = createApp({ desktopToken: "desktop-token", computerToken, rendererOrigin: "http://localhost:5173" });
  const fetchFn: typeof fetch = async (input, init) => app.request(input, init);
  return { app, fetchFn };
}

describe("connectToServer", () => {
  it("marks the computer as connected on the server", async () => {
    const { app, fetchFn } = inMemoryServer();
    await connectToServer(baseUrl, computerToken, fetchFn);

    const status = await app.request("/desktop/status", { headers: { Authorization: "Bearer desktop-token" } });
    expect(await status.json()).toEqual({ computerConnected: true });
  });

  it("reports a rejected token", async () => {
    const { fetchFn } = inMemoryServer();
    await expect(connectToServer(baseUrl, "wrong-token", fetchFn)).rejects.toThrow(
      "Server 拒绝了 Computer 凭证（401）",
    );
  });

  it("reports an unreachable server", async () => {
    await expect(connectToServer(baseUrl, computerToken)).rejects.toThrow("无法连接 Server（http://127.0.0.1:1）");
  });
});
