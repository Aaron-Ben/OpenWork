import { mkdtemp, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { type AgentId, ComputerAgent, type RoomId, RuntimeSessionId } from "@crew/protocol";
import { createTestApp, TEST_COMPUTER_TOKEN, TEST_DESKTOP_TOKEN, type TestApp } from "@crew/server/testing";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { ServerClient } from "../src/client";
import { prepareAgent, prepareRuntime } from "../src/home";
import { AgentRunner } from "../src/runner";
import { FakeEngine } from "./support/fake-engine";

// 真实的 Server 应用（内存中，临时数据库）加上按脚本回复的假 Engine。

let t: TestApp;
let root: string;
let agent: ComputerAgent;
let client: ServerClient;

beforeEach(async () => {
  t = await createTestApp();
  root = await realpath(await mkdtemp(join(tmpdir(), "crew-runner-test-")));
  const created = await t.request("/desktop/agents", {
    method: "POST",
    headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}`, "Content-Type": "application/json" },
    body: JSON.stringify({
      displayName: "Alice",
      handle: "alice",
      persona: "代码审查者",
      model: "opencode-go/deepseek-v4-pro",
    }),
  });
  agent = ComputerAgent.parse(await created.json());
  client = new ServerClient("http://127.0.0.1:1", TEST_COMPUTER_TOKEN, t.fetch);
});

afterEach(async () => {
  await t.close();
  await rm(root, { recursive: true, force: true });
});

async function newRunner(engine: FakeEngine) {
  const runtime = await prepareRuntime(join(root, "crew"), RuntimeSessionId.parse("session-1"));
  const layout = await prepareAgent(runtime, agent);
  return new AgentRunner({
    agent,
    server: client,
    engine,
    layout,
    confinement: { home: root, writable: [], homeReadable: [] },
    env: {},
  });
}

async function send(roomId: RoomId, body: string) {
  const response = await t.request(`/desktop/rooms/${roomId}/messages`, {
    method: "POST",
    headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}`, "Content-Type": "application/json" },
    body: JSON.stringify({ body }),
  });
  expect(response.status).toBe(201);
}

const unread = async (agentId: AgentId) => (await client.readInbox(agentId)).flatMap((room) => room.messages);
/** Agent 的状态：界面从 Agent 列表读到的样子，由运行记录推出。 */
async function status(agentId: AgentId) {
  const response = await t.request("/desktop/agents", { headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}` } });
  const agents = (await response.json()) as Array<{ id: string; status: unknown }>;
  return agents.find((candidate) => candidate.id === agentId)?.status;
}

async function runs(agentId: AgentId) {
  const response = await t.request(`/desktop/runs?agentId=${agentId}`, {
    headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}` },
  });
  return (await response.json()) as Array<{ id: string; outcome: string; error: string | null; triggers: unknown }>;
}

describe("AgentRunner", () => {
  it("runs a turn with the unread messages, then acknowledges them and reports idle", async () => {
    const engine = new FakeEngine([{ ok: true, sessionId: "ses_1" }]);
    const runner = await newRunner(engine);
    await send(agent.roomId, "帮我看下这个函数");

    runner.wake();
    await runner.idle();

    expect(engine.requests).toHaveLength(1);
    expect(engine.requests[0]?.prompt).toContain("User (user): 帮我看下这个函数");
    expect(engine.requests[0]?.sessionId).toBeUndefined();
    expect(await unread(agent.id)).toEqual([]);
    expect(await status(agent.id)).toEqual({ state: "idle" });
  });

  it("tells the agent to read its memory in a new session, and not when it continues the last one", async () => {
    const engine = new FakeEngine([
      { ok: true, sessionId: "ses_1" },
      { ok: true, sessionId: "ses_1" },
    ]);
    const runner = await newRunner(engine);
    await send(agent.roomId, "第一条");
    runner.wake();
    await runner.idle();
    await send(agent.roomId, "第二条");
    runner.wake();
    await runner.idle();

    expect(engine.requests[0]?.prompt).toContain("This is a new session");
    expect(engine.requests[1]?.sessionId).toBe("ses_1");
    expect(engine.requests[1]?.prompt).not.toContain("This is a new session");
  });

  it("records each turn: what woke it, the engine's steps in order, and the result", async () => {
    const at = "2026-10-05T12:00:00.000Z";
    const engine = new FakeEngine([{ ok: true, sessionId: "ses_1" }]);
    engine.events = [
      { kind: "step", at },
      { kind: "tool", at, tool: "bash", title: "ls", input: "{}", output: "a.txt", durationMs: 3, failed: false },
      {
        kind: "step_end",
        at,
        usage: { input: 10, output: 2, reasoning: 0, cacheRead: 0, cacheWrite: 0, cost: 0.001 },
      },
    ];
    const runner = await newRunner(engine);
    await send(agent.roomId, "第一条");
    await send(agent.roomId, "第二条");
    runner.wake();
    await runner.idle();

    const [run] = await runs(agent.id);
    expect(run).toMatchObject({ outcome: "succeeded", triggers: [{ roomId: agent.roomId, fromSeq: 1, toSeq: 2 }] });
    const response = await t.request(`/desktop/runs/${run?.id}`, {
      headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}` },
    });
    const detail = (await response.json()) as { prompt: string; steps: number; events: Array<{ kind: string }> };
    expect(detail.prompt).toBe(engine.requests[0]?.prompt);
    expect(detail.events.map((event) => event.kind)).toEqual(["step", "tool", "step_end"]);
    expect(detail.steps).toBe(1);
  });

  it("continues the saved session on the next turn", async () => {
    const engine = new FakeEngine([
      { ok: true, sessionId: "ses_1" },
      { ok: true, sessionId: "ses_1" },
    ]);
    const runner = await newRunner(engine);
    await send(agent.roomId, "第一条");
    runner.wake();
    await runner.idle();
    await send(agent.roomId, "第二条");
    runner.wake();
    await runner.idle();

    expect(engine.requests[1]?.sessionId).toBe("ses_1");
    expect(engine.requests[1]?.prompt).toContain("第二条");
    expect(engine.requests[1]?.prompt).not.toContain("第一条");
  });

  it("keeps the messages and reports the reason when the turn fails, then retries them on the next wake", async () => {
    const engine = new FakeEngine([
      { ok: false, error: { kind: "unauthenticated", message: "OpenCode 未登录" } },
      { ok: true, sessionId: "ses_2" },
    ]);
    const runner = await newRunner(engine);
    await send(agent.roomId, "在吗");

    runner.wake();
    await runner.idle();
    expect(await status(agent.id)).toEqual({ state: "error", reason: "OpenCode 未登录", roomIds: [agent.roomId] });
    expect((await unread(agent.id)).map((m) => m.body)).toEqual(["在吗"]);

    await send(agent.roomId, "登录好了");
    runner.wake();
    await runner.idle();
    expect(engine.requests[1]?.prompt).toContain("在吗");
    expect(engine.requests[1]?.prompt).toContain("登录好了");
    expect(await status(agent.id)).toEqual({ state: "idle" });
  });

  it("merges wakes that arrive during a turn into one more turn", async () => {
    const engine = new FakeEngine([], true);
    const runner = await newRunner(engine);
    await send(agent.roomId, "第一条");
    runner.wake();
    for (let i = 0; i < 100 && engine.requests.length === 0; i++) await new Promise((r) => setTimeout(r, 10));
    expect(await status(agent.id)).toMatchObject({ state: "working", roomIds: [agent.roomId] });

    await send(agent.roomId, "第二条");
    runner.wake();
    await send(agent.roomId, "第三条");
    runner.wake();
    engine.finishTurn();
    for (let i = 0; i < 100 && engine.requests.length < 2; i++) await new Promise((r) => setTimeout(r, 10));
    engine.finishTurn();
    await runner.idle();

    expect(engine.requests).toHaveLength(2);
    expect(engine.requests[1]?.prompt).toContain("第二条");
    expect(engine.requests[1]?.prompt).toContain("第三条");
    expect(engine.requests[1]?.prompt).not.toContain("第一条");
  });

  it("reports an error instead of staying in working when the engine throws", async () => {
    const engine = new FakeEngine([]);
    engine.runTurn = async () => {
      throw new Error("登录文件读不了");
    };
    const runner = await newRunner(engine);
    await send(agent.roomId, "在吗");

    runner.wake();
    await runner.idle();
    expect(await status(agent.id)).toEqual({
      state: "error",
      reason: "处理失败：登录文件读不了",
      roomIds: [agent.roomId],
    });
    expect((await unread(agent.id)).map((m) => m.body)).toEqual(["在吗"]);
  });

  it("does nothing when the inbox is empty", async () => {
    const engine = new FakeEngine([]);
    const runner = await newRunner(engine);
    runner.wake();
    await runner.idle();
    expect(engine.requests).toHaveLength(0);
    expect(await status(agent.id)).toEqual({ state: "idle" });
  });

  it("stops a running turn without acknowledging, and records it as cancelled", async () => {
    const engine = new FakeEngine([], true);
    const runner = await newRunner(engine);
    await send(agent.roomId, "长任务");
    runner.wake();
    for (let i = 0; i < 100 && engine.requests.length === 0; i++) await new Promise((r) => setTimeout(r, 10));

    await runner.stop();
    expect(engine.requests[0]?.signal.aborted).toBe(true);
    expect((await unread(agent.id)).map((m) => m.body)).toEqual(["长任务"]);
    expect(await status(agent.id)).toEqual({ state: "idle" });
    expect((await runs(agent.id)).map((run) => run.outcome)).toEqual(["cancelled"]);

    runner.wake();
    await runner.idle();
    expect(engine.requests).toHaveLength(1);
  });
});
