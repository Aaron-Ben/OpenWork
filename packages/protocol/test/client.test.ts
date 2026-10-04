import { describe, expect, it } from "vitest";
import { ZodError } from "zod";
import { AgentId, ApiClient, ApiError, api, endpointPath, errorMessage, MessageWindow, RoomId } from "../src";

// 按契约调用 Server 的客户端。用一个记录请求、返回预设响应的 fetch 代替 Server。

const agentId = AgentId.parse("2f8c0b6e-3a1d-4c5e-9f7a-1b2c3d4e5f60");

function fakeFetch(response: Response) {
  const calls: Array<{ url: string; method: string; headers: Headers; body: string | undefined }> = [];
  const fetchFn: typeof fetch = async (input, init) => {
    calls.push({
      url: String(input),
      method: init?.method ?? "GET",
      headers: new Headers(init?.headers),
      body: typeof init?.body === "string" ? init.body : undefined,
    });
    return response;
  };
  return { fetchFn, calls };
}

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });

describe("ApiClient.call", () => {
  it("fills path parameters, sends the token and body, and returns the validated response", async () => {
    const { fetchFn, calls } = fakeFetch(json({ token: "secret" }));
    const client = new ApiClient({ baseUrl: "http://server", token: "t", fetch: fetchFn });

    expect(await client.call(api.computer.issueAgentToken, { params: { agentId } })).toEqual({ token: "secret" });
    expect(calls[0]).toMatchObject({ url: `http://server/computer/agents/${agentId}/token`, method: "POST" });
    expect(calls[0]?.headers.get("Authorization")).toBe("Bearer t");
  });

  it("sends a JSON body and returns nothing for an endpoint without a response", async () => {
    const { fetchFn, calls } = fakeFetch(new Response(null, { status: 204 }));
    const client = new ApiClient({ baseUrl: "http://server", token: "t", fetch: fetchFn });

    expect(await client.call(api.computer.reportModels, { body: { models: ["a/b"] } })).toBeUndefined();
    expect(calls[0]?.body).toBe(JSON.stringify({ models: ["a/b"] }));
    expect(calls[0]?.headers.get("Content-Type")).toBe("application/json");
  });

  it("puts defined query parameters in the URL and leaves out undefined ones", async () => {
    const { fetchFn, calls } = fakeFetch(json([]));
    const client = new ApiClient({ baseUrl: "http://server", token: "t", fetch: fetchFn });
    const roomId = RoomId.parse("7a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d");

    await client.call(api.desktop.listMessages, { params: { roomId }, query: { after: 12, before: undefined } });
    expect(calls[0]?.url).toBe(`http://server/desktop/rooms/${roomId}/messages?after=12`);
  });

  it("parses query strings into numbers and rejects after together with before", () => {
    expect(MessageWindow.parse({ after: "12", limit: "50" })).toEqual({ after: 12, limit: 50 });
    expect(MessageWindow.safeParse({ after: "-1" }).success).toBe(false);
    expect(MessageWindow.safeParse({ limit: "0" }).success).toBe(false);
    expect(MessageWindow.safeParse({ after: "1", before: "5" }).success).toBe(false);
  });

  it("throws the server's reason with the status when the server refuses", async () => {
    const { fetchFn } = fakeFetch(json({ error: "名字不能为空" }, 400));
    const client = new ApiClient({ baseUrl: "http://server", token: "t", fetch: fetchFn });

    const error = await client
      .call(api.desktop.createAgent, { body: { displayName: "", persona: "x", model: "a/b", handle: "x" } })
      .catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({ status: 400, message: "名字不能为空" });
  });

  it("rejects a response that does not match the contract", async () => {
    const { fetchFn } = fakeFetch(json({ token: "" }));
    const client = new ApiClient({ baseUrl: "http://server", token: "t", fetch: fetchFn });

    await expect(client.call(api.computer.issueAgentToken, { params: { agentId } })).rejects.toBeInstanceOf(ZodError);
  });
});

describe("ApiClient with the global fetch", () => {
  it("calls fetch the way browsers require, not as a method of the client", async () => {
    const original = globalThis.fetch;
    // 浏览器的 fetch 只能以 window（这里是 globalThis）为 this 调用，否则抛出 Illegal invocation。
    globalThis.fetch = async function (this: unknown) {
      if (this !== undefined && this !== globalThis) throw new TypeError("Illegal invocation");
      return json([]);
    };
    try {
      const client = new ApiClient({ baseUrl: "http://server", token: "t" });
      expect(await client.call(api.desktop.listModels)).toEqual([]);
    } finally {
      globalThis.fetch = original;
    }
  });
});

describe("endpointPath", () => {
  it("encodes parameter values and reports a missing one", () => {
    expect(endpointPath("/rooms/:roomId/messages", { roomId: "a b/c" })).toBe("/rooms/a%20b%2Fc/messages");
    expect(() => endpointPath("/rooms/:roomId", {})).toThrow("缺少路径参数 roomId");
  });
});

describe("errorMessage", () => {
  it("uses the server's reason", () => {
    expect(errorMessage({ error: "名字不能为空" }, 400)).toBe("名字不能为空");
  });

  it("falls back to the status code", () => {
    expect(errorMessage("Internal Server Error", 500)).toBe("Server 返回 500");
    expect(errorMessage(undefined, 502)).toBe("Server 返回 502");
  });
});
