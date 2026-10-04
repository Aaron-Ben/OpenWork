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
  handle: string;
}

/** handle 默认取名字的小写，测试里的名字各不相同。 */
async function newAgent(displayName = "Alice", handle = displayName.toLowerCase()): Promise<CreatedAgent> {
  const response = await desktop("/desktop/agents", "POST", {
    displayName,
    handle,
    persona: "严谨的代码审查者",
    model: "opencode-go/deepseek-v4-pro",
  });
  expect(response.status).toBe(201);
  return (await response.json()) as CreatedAgent;
}

interface Group {
  id: RoomId;
  name: string;
  agentIds: AgentId[];
}

async function newGroup(name: string, agents: CreatedAgent[]): Promise<Group> {
  const response = await desktop("/desktop/groups", "POST", { name, agentIds: agents.map((agent) => agent.id) });
  expect(response.status).toBe(201);
  return (await response.json()) as Group;
}

interface Inbox {
  roomId: RoomId;
  kind: string;
  name: string | null;
  members: Array<{ kind: string; displayName: string; handle: string | null }>;
  messages: Array<{ seq: number; body: string; mentionsYou: boolean; author: { handle: string | null } }>;
}

async function readInbox(agentId: AgentId): Promise<Inbox[]> {
  const response = await computer(`/computer/agents/${agentId}/inbox`, "POST");
  expect(response.status).toBe(200);
  return (await response.json()) as Inbox[];
}

async function acknowledge(agentId: AgentId) {
  expect((await computer(`/computer/agents/${agentId}/inbox/ack`, "POST")).status).toBe(204);
}

async function reply(token: string, roomId: RoomId, body: string) {
  const response = await call(token, "/agent/reply", "POST", { roomId, body });
  expect(response.status).toBe(200);
  return (await response.json()) as
    | { outcome: "posted"; seq: number }
    | { outcome: "held"; newMessages: Array<{ seq: number; body: string }>; omitted: number };
}

async function listMessages(roomId: RoomId, query = "") {
  const response = await desktop(`/desktop/rooms/${roomId}/messages${query}`);
  expect(response.status).toBe(200);
  return (await response.json()) as Array<{ seq: number; body: string }>;
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
    const response = await desktop("/desktop/agents", "POST", {
      displayName: "  ",
      handle: "blank",
      persona: "x",
      model: "m",
    });
    expect(response.status).toBe(400);
    expect(await response.json()).toEqual({ error: "名字不能为空" });
  });

  it("reject a handle that is taken or not in the allowed format", async () => {
    await newAgent("Taken");
    const again = await desktop("/desktop/agents", "POST", {
      displayName: "Taken again",
      handle: "taken",
      persona: "x",
      model: "m",
    });
    expect(again.status).toBe(409);
    expect(await again.json()).toEqual({ error: "handle @taken 已被使用" });

    const upper = await desktop("/desktop/agents", "POST", {
      displayName: "X",
      handle: "Upper",
      persona: "x",
      model: "m",
    });
    expect(upper.status).toBe(400);
  });
});

describe("groups", () => {
  it("are created with the chosen agents, listed, and announced to the desktop", async () => {
    const alice = await newAgent("GroupAlice");
    const bob = await newAgent("GroupBob");
    const events = await collect<DesktopEvent>(t.ctx.events.desktop, async () => {
      const group = await newGroup("评审", [alice, bob]);
      expect(group.agentIds.toSorted()).toEqual([alice.id, bob.id].toSorted());
    });
    expect(events).toEqual([{ type: "rooms" }]);

    const groups = (await (await desktop("/desktop/groups")).json()) as Group[];
    expect(groups.map((group) => group.name)).toContain("评审");
  });

  it("reject an empty member list, a blank name and an unknown agent", async () => {
    const alice = await newAgent("Picky");
    expect((await desktop("/desktop/groups", "POST", { name: "空", agentIds: [] })).status).toBe(400);
    expect((await desktop("/desktop/groups", "POST", { name: " ", agentIds: [alice.id] })).status).toBe(400);
    const ghost = "00000000-0000-4000-8000-000000000000";
    expect((await desktop("/desktop/groups", "POST", { name: "幽灵", agentIds: [ghost] })).status).toBe(404);
  });

  it("let a new member see only the messages sent after it joined", async () => {
    const alice = await newAgent("Early");
    const bob = await newAgent("Late");
    const group = await newGroup("先来后到", [alice]);
    await sendAsUser(group.id, "Bob 加入前");

    const added = await desktop(`/desktop/groups/${group.id}/members`, "POST", { agentIds: [bob.id] });
    expect(added.status).toBe(200);
    expect(((await added.json()) as Group).agentIds).toContain(bob.id);
    await sendAsUser(group.id, "Bob 加入后");

    const inbox = await readInbox(bob.id);
    expect(inbox.flatMap((room) => room.messages.map((m) => m.body))).toEqual(["Bob 加入后"]);
  });

  it("cannot take members through a direct room id", async () => {
    const alice = await newAgent("Direct");
    const bob = await newAgent("Intruder");
    const response = await desktop(`/desktop/groups/${alice.roomId}/members`, "POST", { agentIds: [bob.id] });
    expect(response.status).toBe(404);
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

  it("from the user wake every agent in a group", async () => {
    const alice = await newAgent("AllAlice");
    const bob = await newAgent("AllBob");
    const group = await newGroup("全员", [alice, bob]);
    const events = await collect<ComputerEvent>(t.ctx.events.computer, () => sendAsUser(group.id, "大家好"));
    expect(events).toHaveLength(2);
    expect(events).toEqual(
      expect.arrayContaining([
        { type: "agent.wake", agentId: alice.id },
        { type: "agent.wake", agentId: bob.id },
      ]),
    );
  });

  it("from an agent wake only the members it mentions", async () => {
    const alice = await newAgent("Speaker", "speaker");
    const bob = await newAgent("Named", "named");
    const carol = await newAgent("Bystander", "bystander");
    const outsider = await newAgent("Outside", "outside");
    const group = await newGroup("点名", [alice, bob, carol]);
    const token = await agentToken(alice.id);

    const quiet = await collect<ComputerEvent>(t.ctx.events.computer, () => reply(token, group.id, "我做完了"));
    expect(quiet).toEqual([]);

    const named = await collect<ComputerEvent>(t.ctx.events.computer, () =>
      reply(token, group.id, "@Named 请看一下，`@bystander` 不算，@outside 不在群里，@speaker 是我自己"),
    );
    expect(named).toEqual([{ type: "agent.wake", agentId: bob.id }]);
    expect(outsider.id).not.toBe(bob.id);
  });

  it("are listed by window: the latest batch, after a position, or before one", async () => {
    const agent = await newAgent("Window");
    for (let i = 1; i <= 5; i++) await sendAsUser(agent.roomId, `第 ${i} 条`);

    const seqs = async (query: string) => (await listMessages(agent.roomId, query)).map((m) => m.seq);
    expect(await seqs("")).toEqual([1, 2, 3, 4, 5]);
    expect(await seqs("?limit=2")).toEqual([4, 5]);
    expect(await seqs("?after=3")).toEqual([4, 5]);
    expect(await seqs("?after=1&limit=2")).toEqual([2, 3]);
    expect(await seqs("?before=4&limit=2")).toEqual([2, 3]);
    expect((await desktop(`/desktop/rooms/${agent.roomId}/messages?after=1&before=3`)).status).toBe(400);
    expect((await desktop(`/desktop/rooms/${agent.roomId}/messages?limit=abc`)).status).toBe(400);
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
  it("returns messages after the read position until they are acknowledged", async () => {
    const agent = await newAgent("Inbox");
    await sendAsUser(agent.roomId, "一");
    await sendAsUser(agent.roomId, "二");

    const seqs = async () => (await readInbox(agent.id)).map((room) => room.messages.map((m) => m.seq));
    expect(await seqs()).toEqual([[1, 2]]);
    // 没有确认时再读一次，仍是同样的消息：Turn 失败后重新处理。
    expect(await seqs()).toEqual([[1, 2]]);

    await acknowledge(agent.id);
    expect(await readInbox(agent.id)).toEqual([]);

    // 确认只推进到读取过的位置，之后的新消息仍是未读。
    await sendAsUser(agent.roomId, "三");
    await acknowledge(agent.id);
    expect(await seqs()).toEqual([[3]]);
  });

  it("names the room, lists its members and marks messages that mention the agent", async () => {
    const alice = await newAgent("RosterAlice", "roster-alice");
    const bob = await newAgent("RosterBob", "roster-bob");
    const group = await newGroup("名册", [alice, bob]);
    await sendAsUser(group.id, "大家看看");
    await sendAsUser(group.id, "@roster-bob 你来");

    const [room] = await readInbox(bob.id);
    expect(room).toMatchObject({ roomId: group.id, kind: "group", name: "名册" });
    expect(room?.members.map((m) => [m.kind, m.displayName, m.handle])).toEqual([
      ["user", "User", null],
      ["agent", "RosterAlice", "roster-alice"],
      ["agent", "RosterBob", "roster-bob"],
    ]);
    expect(room?.messages.map((m) => m.mentionsYou)).toEqual([false, true]);

    const [direct] = await readInbox(alice.id);
    expect(direct?.messages.map((m) => m.mentionsYou)).toEqual([false, false]);
  });
});

describe("agent replies", () => {
  it("are written as the agent and do not wake the agent itself", async () => {
    const agent = await newAgent("Replier");
    const token = await agentToken(agent.id);
    const events = await collect<ComputerEvent>(t.ctx.events.computer, async () => {
      expect(await reply(token, agent.roomId, "我看完了")).toMatchObject({ outcome: "posted", seq: 1 });
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

  it("are held when someone else wrote after what the agent was given, until it has seen those messages", async () => {
    const agent = await newAgent("Held");
    const token = await agentToken(agent.id);
    await sendAsUser(agent.roomId, "帮我看看这个");
    await readInbox(agent.id);
    await sendAsUser(agent.roomId, "算了，换个问题");

    const events = await collect<DesktopEvent>(t.ctx.events.desktop, async () => {
      expect(await reply(token, agent.roomId, "好的，我来看")).toEqual({
        outcome: "held",
        newMessages: [expect.objectContaining({ seq: 2, body: "算了，换个问题" })],
        omitted: 0,
      });
    });
    expect(events).toEqual([]);
    expect((await listMessages(agent.roomId)).map((m) => m.body)).toEqual(["帮我看看这个", "算了，换个问题"]);

    // 看过新消息之后再发，发得出去；它自己的消息不会让下一次回复被拦下。
    expect(await reply(token, agent.roomId, "好，换个问题")).toMatchObject({ outcome: "posted", seq: 3 });
    expect(await reply(token, agent.roomId, "补充一句")).toMatchObject({ outcome: "posted", seq: 4 });
  });

  it("show many new messages oldest first, a batch at a time, without skipping the rest", async () => {
    const agent = await newAgent("Flooded");
    const token = await agentToken(agent.id);
    for (let i = 1; i <= 23; i++) await sendAsUser(agent.roomId, `消息 ${i}`);

    const first = await reply(token, agent.roomId, "来了");
    if (first.outcome !== "held") throw new Error("应当被拦下");
    expect(first.newMessages.map((m) => m.seq)).toEqual(Array.from({ length: 20 }, (_, i) => i + 1));
    expect(first.omitted).toBe(3);

    // Turn 在这里结束：没有返回的 3 条仍是未读，下一轮会看到。
    await acknowledge(agent.id);
    expect((await readInbox(agent.id)).flatMap((room) => room.messages.map((m) => m.seq))).toEqual([21, 22, 23]);

    const second = await reply(token, agent.roomId, "来了");
    expect(second).toMatchObject({ outcome: "posted" });
  });

  it("show the rest of a large batch on the next reply in the same turn", async () => {
    const agent = await newAgent("Paged");
    const token = await agentToken(agent.id);
    for (let i = 1; i <= 23; i++) await sendAsUser(agent.roomId, `消息 ${i}`);

    expect((await reply(token, agent.roomId, "来了")).outcome).toBe("held");
    const second = await reply(token, agent.roomId, "来了");
    if (second.outcome !== "held") throw new Error("应当再次被拦下");
    expect(second.newMessages.map((m) => m.seq)).toEqual([21, 22, 23]);
    expect(second.omitted).toBe(0);
    expect((await reply(token, agent.roomId, "来了")).outcome).toBe("posted");
  });

  it("are not shown again after the turn is acknowledged, nor are messages shown by a hold", async () => {
    const agent = await newAgent("Acked");
    const token = await agentToken(agent.id);
    await sendAsUser(agent.roomId, "问题");
    await readInbox(agent.id);
    await sendAsUser(agent.roomId, "补充");
    expect((await reply(token, agent.roomId, "回答")).outcome).toBe("held");
    expect((await reply(token, agent.roomId, "回答")).outcome).toBe("posted");
    await acknowledge(agent.id);

    expect(await readInbox(agent.id)).toEqual([]);
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
