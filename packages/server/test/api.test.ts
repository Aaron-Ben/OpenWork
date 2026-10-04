import { type AgentId, type ComputerEvent, DesktopEvent, RoomId, runEventStream } from "@crew/protocol";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createTestApp, TEST_COMPUTER_TOKEN, TEST_DESKTOP_TOKEN, type TestApp } from "./support/app";

// 业务接口的集成测试：真实的 PostgreSQL 临时库，应用在内存中处理请求。

let t: TestApp;
beforeAll(async () => {
  t = await createTestApp();
});
afterAll(async () => {
  await t.close();
});

function call(token: string, path: string, method = "GET", body?: unknown) {
  return t.request(path, {
    method,
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}
const desktop = (path: string, method?: string, body?: unknown) => call(TEST_DESKTOP_TOKEN, path, method, body);
const computer = (path: string, method?: string, body?: unknown) => call(TEST_COMPUTER_TOKEN, path, method, body);

interface CreatedAgent {
  id: AgentId;
  roomId: RoomId;
}

async function newAgent(displayName = "Alice"): Promise<CreatedAgent> {
  const response = await desktop("/desktop/agents", "POST", {
    displayName,
    persona: "严谨的代码审查者",
    model: "opencode-go/deepseek-v4-pro",
  });
  expect(response.status).toBe(201);
  return (await response.json()) as CreatedAgent;
}

async function agentToken(agentId: AgentId): Promise<string> {
  const response = await computer(`/computer/agents/${agentId}/token`, "POST");
  return ((await response.json()) as { token: string }).token;
}

async function sendAsUser(roomId: RoomId, body: string) {
  return desktop(`/desktop/rooms/${roomId}/messages`, "POST", { body });
}

/** 收集一个事件通道在 `run` 期间发布的事件。 */
async function collect<T>(channel: { subscribe(l: (e: T) => void): () => void }, run: () => Promise<unknown>) {
  const events: T[] = [];
  const unsubscribe = channel.subscribe((event) => events.push(event));
  await run();
  unsubscribe();
  return events;
}

describe("agents", () => {
  it("are created with a direct room and listed with an idle status", async () => {
    const created = await newAgent("Lister");
    const list = (await (await desktop("/desktop/agents")).json()) as Array<CreatedAgent & { status: unknown }>;
    const listed = list.find((agent) => agent.id === created.id);
    expect(listed).toMatchObject({ roomId: created.roomId, status: { state: "idle" } });
  });

  it("notify both the desktop and the computer when one is created", async () => {
    const desktopEvents = await collect<DesktopEvent>(t.ctx.events.desktop, () => newAgent("Notified"));
    expect(desktopEvents).toContainEqual({ type: "agents" });
  });

  it("reject a blank name with a JSON error", async () => {
    const response = await desktop("/desktop/agents", "POST", { displayName: "  ", persona: "x", model: "m" });
    expect(response.status).toBe(400);
    expect(await response.json()).toEqual({ error: "名字不能为空" });
  });
});

describe("messages", () => {
  it("get consecutive sequence numbers and are listed with their authors", async () => {
    const agent = await newAgent("Seq");
    await sendAsUser(agent.roomId, "第一条");
    await sendAsUser(agent.roomId, "第二条");
    const messages = (await (await desktop(`/desktop/rooms/${agent.roomId}/messages`)).json()) as Array<{
      seq: number;
      body: string;
      author: { kind: string; displayName: string };
    }>;
    expect(messages.map((m) => [m.seq, m.body, m.author.kind, m.author.displayName])).toEqual([
      [1, "第一条", "user", "User"],
      [2, "第二条", "user", "User"],
    ]);
  });

  it("keep sequence numbers gap-free under concurrent writes", async () => {
    const agent = await newAgent("Concurrent");
    const responses = await Promise.all(Array.from({ length: 10 }, (_, i) => sendAsUser(agent.roomId, `并发 ${i}`)));
    const seqs = await Promise.all(responses.map(async (r) => ((await r.json()) as { seq: number }).seq));
    expect(seqs.sort((a, b) => a - b)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
  });

  it("from the user wake the agent and refresh the room", async () => {
    const agent = await newAgent("Wake");
    const computerEvents = await collect<ComputerEvent>(t.ctx.events.computer, () => sendAsUser(agent.roomId, "在吗"));
    expect(computerEvents).toEqual([{ type: "agent.wake", agentId: agent.id }]);
  });

  it("reject a blank body and an overlong body", async () => {
    const agent = await newAgent("Body");
    expect((await sendAsUser(agent.roomId, "   ")).status).toBe(400);
    expect((await sendAsUser(agent.roomId, "长".repeat(20_001))).status).toBe(400);
  });

  it("return 404 for a room that does not exist", async () => {
    const response = await sendAsUser(RoomId.parse("00000000-0000-4000-8000-000000000000"), "你好");
    expect(response.status).toBe(404);
  });
});

describe("inbox", () => {
  it("returns messages after the read position, and ack moves it forward only", async () => {
    const agent = await newAgent("Inbox");
    await sendAsUser(agent.roomId, "一");
    await sendAsUser(agent.roomId, "二");

    const inbox = async () =>
      (await (await computer(`/computer/agents/${agent.id}/inbox`)).json()) as Array<{
        roomId: string;
        messages: Array<{ seq: number }>;
      }>;
    expect((await inbox()).map((room) => room.messages.map((m) => m.seq))).toEqual([[1, 2]]);

    const ack = (seq: number) =>
      computer(`/computer/agents/${agent.id}/inbox/ack`, "POST", { acks: [{ roomId: agent.roomId, seq }] });
    expect((await ack(2)).status).toBe(204);
    expect(await inbox()).toEqual([]);

    expect((await ack(1)).status).toBe(204);
    expect(await inbox()).toEqual([]);

    expect((await ack(9)).status).toBe(400);
  });
});

describe("agent replies", () => {
  it("are written as the agent and do not wake the agent itself", async () => {
    const agent = await newAgent("Replier");
    const token = await agentToken(agent.id);
    const events = await collect<ComputerEvent>(t.ctx.events.computer, async () => {
      const response = await call(token, "/agent/reply", "POST", { roomId: agent.roomId, body: "我看完了" });
      expect(response.status).toBe(201);
    });
    expect(events).toEqual([]);

    const messages = (await (await desktop(`/desktop/rooms/${agent.roomId}/messages`)).json()) as Array<{
      author: { kind: string; displayName: string };
    }>;
    expect(messages.at(-1)?.author).toMatchObject({ kind: "agent", displayName: "Replier" });
  });

  it("are rejected in a room the agent is not a member of", async () => {
    const alice = await newAgent("Outsider");
    const bob = await newAgent("Owner");
    const token = await agentToken(alice.id);
    const response = await call(token, "/agent/reply", "POST", { roomId: bob.roomId, body: "我来插话" });
    expect(response.status).toBe(403);
  });

  it("stop working with a replaced token", async () => {
    const agent = await newAgent("Rotated");
    const old = await agentToken(agent.id);
    await agentToken(agent.id);
    const response = await call(old, "/agent/reply", "POST", { roomId: agent.roomId, body: "旧凭证" });
    expect(response.status).toBe(401);
  });
});

describe("status and models", () => {
  it("show a reported status in the agent list and notify the desktop", async () => {
    const agent = await newAgent("Busy");
    const events = await collect<DesktopEvent>(t.ctx.events.desktop, async () => {
      const response = await computer(`/computer/agents/${agent.id}/status`, "POST", { state: "working" });
      expect(response.status).toBe(204);
    });
    expect(events).toEqual([{ type: "agents" }]);

    const list = (await (await desktop("/desktop/agents")).json()) as Array<{ id: string; status: unknown }>;
    expect(list.find((a) => a.id === agent.id)?.status).toEqual({ state: "working" });
  });

  it("serve the model list reported by the computer and tell the desktop to refresh it", async () => {
    const events = await collect<DesktopEvent>(t.ctx.events.desktop, async () =>
      computer("/computer/models", "POST", { models: ["opencode-go/deepseek-v4-pro", "deepseek/deepseek-v4-pro"] }),
    );
    expect(events).toEqual([{ type: "models" }]);
    expect(await (await desktop("/desktop/models")).json()).toEqual([
      "opencode-go/deepseek-v4-pro",
      "deepseek/deepseek-v4-pro",
    ]);
  });
});

describe("events over SSE", () => {
  it("reach the shared SSE reader, which stops cleanly when aborted", async () => {
    const agent = await newAgent("Reader");
    const received: DesktopEvent[] = [];
    let opened = false;
    const controller = new AbortController();
    const done = runEventStream({
      url: "http://127.0.0.1:1/desktop/events",
      headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}` },
      schema: DesktopEvent,
      onEvent: (event) => received.push(event),
      onOpen: () => {
        opened = true;
      },
      signal: controller.signal,
      fetch: t.fetch,
    });

    for (let i = 0; i < 100 && !opened; i++) await new Promise((r) => setTimeout(r, 10));
    await sendAsUser(agent.roomId, "经过共享读取器");
    for (let i = 0; i < 100 && received.length === 0; i++) await new Promise((r) => setTimeout(r, 10));
    controller.abort();
    await done;

    expect(received).toContainEqual({ type: "room.messages", roomId: agent.roomId });
  });

  it("deliver a room refresh to the desktop after a message is written", async () => {
    const agent = await newAgent("Streamed");
    const response = await desktop("/desktop/events");
    expect(response.headers.get("Content-Type")).toContain("text/event-stream");
    const reader = response.body?.getReader();
    if (!reader) throw new Error("SSE 响应没有 body");

    await sendAsUser(agent.roomId, "推送测试");
    const { value } = await reader.read();
    await reader.cancel();

    expect(new TextDecoder().decode(value)).toContain(JSON.stringify({ type: "room.messages", roomId: agent.roomId }));
  });

  it("end when the server closes its channels, so shutdown does not wait for clients", async () => {
    const response = await desktop("/desktop/events");
    const reader = response.body?.getReader();
    if (!reader) throw new Error("SSE 响应没有 body");

    t.ctx.events.desktop.close();
    const ended = await Promise.race([
      (async () => {
        while (!(await reader.read()).done) {
          // 读掉结束前可能还在路上的数据。
        }
        return true;
      })(),
      new Promise<false>((resolve) => setTimeout(() => resolve(false), 1_000)),
    ]);
    if (!ended) await reader.cancel();
    expect(ended).toBe(true);
  });
});
