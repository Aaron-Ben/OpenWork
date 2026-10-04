import { existsSync } from "node:fs";
import { mkdtemp, readFile, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { type AgentId, ComputerAgent, type RoomId, RuntimeSessionId } from "@crew/protocol";
import { createTestApp, TEST_COMPUTER_TOKEN, TEST_DESKTOP_TOKEN, type TestApp } from "@crew/server/testing";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { ServerClient } from "../src/client";
import { ComputerDaemon } from "../src/daemon";
import { FakeEngine } from "./support/fake-engine";

// Computer 主流程：真实的 Server 应用（内存中）、经内存 fetch 的 SSE 与按脚本回复的假 Engine。

const session = RuntimeSessionId.parse("session-1");

let t: TestApp;
let root: string;
let client: ServerClient;
let daemon: ComputerDaemon | undefined;

beforeEach(async () => {
  t = await createTestApp();
  root = await realpath(await mkdtemp(join(tmpdir(), "crew-daemon-test-")));
  client = new ServerClient("http://127.0.0.1:1", TEST_COMPUTER_TOKEN, t.fetch);
});

afterEach(async () => {
  await daemon?.stop();
  daemon = undefined;
  await t.close();
  await rm(root, { recursive: true, force: true });
});

function desktop(path: string, method = "GET", body?: unknown) {
  return t.app.request(path, {
    method,
    headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}`, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

async function newAgent(displayName: string): Promise<ComputerAgent> {
  const response = await desktop("/desktop/agents", "POST", { displayName, persona: "同事", model: "fake/model" });
  return ComputerAgent.parse(await response.json());
}

const send = (roomId: RoomId, body: string) => desktop(`/desktop/rooms/${roomId}/messages`, "POST", { body });

async function startDaemon(engine: FakeEngine, available = true) {
  daemon = new ComputerDaemon({
    client,
    engine,
    runtimeSessionId: session,
    crewRoot: join(root, "crew"),
    nodeExecutable: "/Applications/Crew.app/Contents/MacOS/Crew",
    shimEntry: "/Applications/Crew.app/Contents/Resources/shim.js",
    probe: async () => (available ? { available: true } : { available: false, reason: "测试中关闭了沙箱" }),
  });
  await daemon.start();
  return daemon;
}

async function until(check: () => boolean | Promise<boolean>) {
  for (let i = 0; i < 300; i++) {
    if (await check()) return;
    await new Promise((r) => setTimeout(r, 10));
  }
  throw new Error("等待超时");
}

/** 本次运行中这个 Agent 的凭证文件（布局见 src/home.ts）。 */
const tokenFile = (agentId: AgentId) => join(root, "crew", "runtime", session, "agents", agentId, "token");

describe("ComputerDaemon", () => {
  it("handles messages that arrived before it started, once it connects", async () => {
    const agent = await newAgent("Early");
    await send(agent.roomId, "启动前的消息");
    const engine = new FakeEngine([]);
    await startDaemon(engine);

    await until(() => engine.requests.length === 1);
    expect(engine.requests[0]?.prompt).toContain("启动前的消息");
  });

  it("wakes the agent when a new message arrives over SSE", async () => {
    const agent = await newAgent("Live");
    const engine = new FakeEngine([]);
    await startDaemon(engine);
    await until(() => existsSync(tokenFile(agent.id)));

    await send(agent.roomId, "在吗");
    await until(() => engine.requests.length === 1);
    expect(engine.requests[0]?.prompt).toContain("在吗");
  });

  it("starts a runner for an agent created after it started", async () => {
    const engine = new FakeEngine([]);
    await startDaemon(engine);
    const agent = await newAgent("Later");
    await until(() => existsSync(tokenFile(agent.id)));

    await send(agent.roomId, "你好，新同事");
    await until(() => engine.requests.length === 1);
  });

  it("gives each agent a token file that the server accepts, and the shim its environment", async () => {
    const agent = await newAgent("Tokened");
    const engine = new FakeEngine([]);
    const started = await startDaemon(engine);
    await until(() => existsSync(tokenFile(agent.id)));

    const token = await readFile(tokenFile(agent.id), "utf8");
    const reply = await t.app.request("/agent/reply", {
      method: "POST",
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body: JSON.stringify({ roomId: agent.roomId, body: "我是 Tokened" }),
    });
    expect(reply.status).toBe(201);

    await send(agent.roomId, "检查环境");
    await until(() => engine.requests.length === 1);
    expect(engine.requests[0]?.env).toMatchObject({
      CREW_SERVER_URL: "http://127.0.0.1:1",
      CREW_TOKEN_FILE: tokenFile(agent.id),
    });
    expect(engine.requests[0]?.env.PATH?.startsWith(join(root, "crew", "runtime", session, "bin"))).toBe(true);
    expect(await readFile(started.shimPath, "utf8")).toContain("/Applications/Crew.app/Contents/Resources/shim.js");
  });

  it("starts no runner and reports the reason when the sandbox is unavailable", async () => {
    const agent = await newAgent("Blocked");
    await send(agent.roomId, "在吗");
    const engine = new FakeEngine([]);
    await startDaemon(engine, false);

    await until(() => t.ctx.state.statusOf(agent.id).state === "error");
    expect(t.ctx.state.statusOf(agent.id)).toEqual({ state: "error", reason: "沙箱不可用：测试中关闭了沙箱" });
    expect(engine.requests).toHaveLength(0);
  });

  it("removes this run's directory when it stops", async () => {
    const engine = new FakeEngine([]);
    const started = await startDaemon(engine);
    const runtimeDir = join(root, "crew", "runtime", session);
    expect(existsSync(runtimeDir)).toBe(true);
    await started.stop();
    daemon = undefined;
    expect(existsSync(runtimeDir)).toBe(false);
  });
});
