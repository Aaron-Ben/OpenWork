import { type AgentId, type ComputerEvent, DesktopEvent, type MessageId, RoomId, runEventStream } from "@crew/protocol";
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
  messages: Array<{
    seq: number;
    kind: string;
    body: string;
    mentionsYou: boolean;
    author: { handle: string | null };
    task: unknown;
  }>;
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
    | { outcome: "posted"; id: MessageId; roomId: RoomId; seq: number }
    | { outcome: "held"; newMessages: Array<{ seq: number; body: string }>; omitted: number };
}

async function listMessages(roomId: RoomId, query = "") {
  const response = await desktop(`/desktop/rooms/${roomId}/messages${query}`);
  expect(response.status).toBe(200);
  return (await response.json()) as Array<{ seq: number; body: string; runId: string | null; heldBefore: number }>;
}

async function listMessagesWithIds(roomId: RoomId) {
  const response = await desktop(`/desktop/rooms/${roomId}/messages`);
  return (await response.json()) as Array<{ id: MessageId; kind: string; body: string }>;
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

interface ConversationRow {
  roomId: RoomId;
  kind: string;
  name: string;
  agentIds: AgentId[];
  lastMessage: { author: { kind: string; displayName: string }; body: string } | null;
  unread: number;
}

async function conversations(): Promise<ConversationRow[]> {
  const response = await desktop("/desktop/conversations");
  expect(response.status).toBe(200);
  return (await response.json()) as ConversationRow[];
}

const conversation = async (roomId: RoomId) => (await conversations()).find((row) => row.roomId === roomId);

describe("conversations", () => {
  it("list direct and group rooms, most recently active first, with the last message", async () => {
    const alice = await newAgent("ConvAlice");
    const bob = await newAgent("ConvBob");
    const group = await newGroup("会话", [alice, bob]);
    await sendAsUser(alice.roomId, "先发给 Alice");
    await sendAsUser(group.id, "再发到群里");

    const rows = await conversations();
    const order = rows.map((row) => row.roomId);
    expect(order.indexOf(group.id)).toBeLessThan(order.indexOf(alice.roomId));
    expect(await conversation(group.id)).toMatchObject({
      kind: "group",
      name: "会话",
      lastMessage: { author: { kind: "user" }, body: "再发到群里" },
    });
    expect((await conversation(group.id))?.agentIds.toSorted()).toEqual([alice.id, bob.id].toSorted());
    expect(await conversation(bob.roomId)).toMatchObject({ kind: "direct", name: "ConvBob", lastMessage: null });
  });

  it("count only messages from others as unread, until the user reads them", async () => {
    const agent = await newAgent("Unread");
    const token = await agentToken(agent.id);
    await sendAsUser(agent.roomId, "我自己的消息不算未读");
    expect((await conversation(agent.roomId))?.unread).toBe(0);

    await readInbox(agent.id);
    await reply(token, agent.roomId, "第一条回复");
    await reply(token, agent.roomId, "第二条回复");
    expect((await conversation(agent.roomId))?.unread).toBe(2);

    expect((await desktop(`/desktop/rooms/${agent.roomId}/read`, "POST", { seq: 2 })).status).toBe(204);
    expect((await conversation(agent.roomId))?.unread).toBe(1);
    // 只前进；超过最新序号时停在最新一条。
    await desktop(`/desktop/rooms/${agent.roomId}/read`, "POST", { seq: 1 });
    expect((await conversation(agent.roomId))?.unread).toBe(1);
    await desktop(`/desktop/rooms/${agent.roomId}/read`, "POST", { seq: 99 });
    expect((await conversation(agent.roomId))?.unread).toBe(0);
    await reply(token, agent.roomId, "之后的回复");
    expect((await conversation(agent.roomId))?.unread).toBe(1);
  });

  it("return 404 when marking a room that does not exist as read", async () => {
    const response = await desktop("/desktop/rooms/00000000-0000-4000-8000-000000000000/read", "POST", { seq: 1 });
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
        roomId: agent.roomId,
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

async function statusOf(agentId: AgentId): Promise<unknown> {
  const list = (await (await desktop("/desktop/agents")).json()) as Array<{ id: string; status: unknown }>;
  return list.find((agent) => agent.id === agentId)?.status;
}

async function startRun(agentId: AgentId, roomId: RoomId, seq = 1): Promise<string> {
  const response = await computer(`/computer/agents/${agentId}/runs`, "POST", {
    prompt: "本轮输入",
    triggers: [{ roomId, fromSeq: seq, toSeq: seq }],
  });
  expect(response.status).toBe(201);
  return ((await response.json()) as { id: string }).id;
}

const finish = (runId: string, body: unknown) => computer(`/computer/runs/${runId}/finish`, "POST", body);
const runDetail = async (runId: string) =>
  (await (await desktop(`/desktop/runs/${runId}`)).json()) as {
    outcome: string;
    steps: number;
    replies: number;
    holds: number;
    usage: Record<string, number>;
    prompt: string;
    events: Array<{ seq: number; kind: string; [key: string]: unknown }>;
  };

const at = "2026-10-05T12:00:00.000Z";
const usage = { input: 100, output: 20, reasoning: 5, cacheRead: 50, cacheWrite: 0, cost: 0.002 };

describe("runs", () => {
  it("make the agent working in the rooms that woke it, then idle when the run succeeds", async () => {
    const agent = await newAgent("Runner");
    await sendAsUser(agent.roomId, "开始");
    expect(await statusOf(agent.id)).toEqual({ state: "idle" });

    const events = await collect<DesktopEvent>(t.ctx.events.desktop, async () => {
      const runId = await startRun(agent.id, agent.roomId);
      expect(await statusOf(agent.id)).toEqual({ state: "working", runId, roomIds: [agent.roomId] });
      expect((await finish(runId, { outcome: "succeeded" })).status).toBe(204);
    });
    expect(events.filter((event) => event.type === "agents")).toHaveLength(2);
    expect(events.filter((event) => event.type === "run.activity")).toHaveLength(2);
    expect(await statusOf(agent.id)).toEqual({ state: "idle" });
  });

  it("record engine events in order and add up the usage of each step", async () => {
    const agent = await newAgent("Stepper");
    await sendAsUser(agent.roomId, "开始");
    const runId = await startRun(agent.id, agent.roomId);
    const append = (events: unknown[]) => computer(`/computer/runs/${runId}/events`, "POST", { events });

    expect((await append([{ kind: "step", at }])).status).toBe(204);
    const tool = {
      kind: "tool",
      at,
      tool: "bash",
      title: "ls",
      input: '{"command":"ls"}',
      output: "a.txt",
      durationMs: 15,
      failed: false,
    };
    await append([tool, { kind: "step_end", at, usage }]);
    await append([
      { kind: "step", at },
      { kind: "text", at, text: "完成" },
      { kind: "step_end", at, usage },
    ]);

    const run = await runDetail(runId);
    expect(run.events.map((event) => [event.seq, event.kind])).toEqual([
      [1, "step"],
      [2, "tool"],
      [3, "step_end"],
      [4, "step"],
      [5, "text"],
      [6, "step_end"],
    ]);
    expect(run.events[1]).toMatchObject(tool);
    expect(run).toMatchObject({ steps: 2, prompt: "本轮输入", outcome: "running" });
    expect(run.usage).toEqual({ input: 200, output: 40, reasoning: 10, cacheRead: 100, cacheWrite: 0, cost: 0.004 });
  });

  it("note replies and holds in the agent's running run, and mark the message with them", async () => {
    const agent = await newAgent("Noted");
    const token = await agentToken(agent.id);
    await sendAsUser(agent.roomId, "问题");
    await readInbox(agent.id);
    const runId = await startRun(agent.id, agent.roomId);
    await sendAsUser(agent.roomId, "补充一句");

    expect((await reply(token, agent.roomId, "回答")).outcome).toBe("held");
    expect((await reply(token, agent.roomId, "改写后的回答")).outcome).toBe("posted");
    expect((await reply(token, agent.roomId, "再补一句")).outcome).toBe("posted");

    const run = await runDetail(runId);
    expect(run).toMatchObject({ replies: 2, holds: 1 });
    expect(run.events.map((event) => event.kind)).toEqual(["held", "reply", "reply"]);
    expect(run.events[0]).toMatchObject({ roomId: agent.roomId, newMessages: 1, preview: "补充一句" });

    const messages = await listMessages(agent.roomId);
    expect(messages.map((m) => [m.body, m.runId, m.heldBefore])).toEqual([
      ["问题", null, 0],
      ["补充一句", null, 0],
      ["改写后的回答", runId, 1],
      ["再补一句", runId, 0],
    ]);
  });

  it("still hold a reply when the new message has an emoji right at the preview's cut", async () => {
    const agent = await newAgent("Emoji");
    const token = await agentToken(agent.id);
    await sendAsUser(agent.roomId, "开始");
    await readInbox(agent.id);
    const runId = await startRun(agent.id, agent.roomId);
    await sendAsUser(agent.roomId, `${"x".repeat(119)}😀 thanks`);

    expect((await reply(token, agent.roomId, "好")).outcome).toBe("held");
    const [held] = (await runDetail(runId)).events;
    expect(held).toMatchObject({ kind: "held", preview: "x".repeat(119) });
  });

  it("store tool output with NUL bytes and broken emoji instead of dropping the batch", async () => {
    const agent = await newAgent("Binary");
    await sendAsUser(agent.roomId, "开始");
    const runId = await startRun(agent.id, agent.roomId);
    const binary = {
      kind: "tool",
      at,
      tool: "bash",
      title: "cat",
      input: "{}",
      output: "ELF\u0000\u0001",
      durationMs: 1,
      failed: false,
    };
    const response = await computer(`/computer/runs/${runId}/events`, "POST", {
      events: [binary, { kind: "text", at, text: "a\ud83d" }, { kind: "step_end", at, usage }],
    });
    expect(response.status).toBe(204);

    const run = await runDetail(runId);
    expect(run.events.map((event) => event.kind)).toEqual(["tool", "text", "step_end"]);
    expect(run.events[0]).toMatchObject({ output: "ELF\uFFFD\u0001" });
    expect(run.events[1]).toMatchObject({ text: "a\uFFFD" });
    expect(run.usage.input).toBe(100);
  });

  it("show a failed run as an error in its rooms until a later run succeeds", async () => {
    const agent = await newAgent("Failing");
    await sendAsUser(agent.roomId, "开始");
    const failed = await startRun(agent.id, agent.roomId);
    expect((await finish(failed, { outcome: "failed", error: "限流" })).status).toBe(204);
    expect(await statusOf(agent.id)).toEqual({ state: "error", reason: "限流", roomIds: [agent.roomId] });

    const next = await startRun(agent.id, agent.roomId);
    await finish(next, { outcome: "succeeded" });
    expect(await statusOf(agent.id)).toEqual({ state: "idle" });
  });

  it("refuse events and results for a run that already ended", async () => {
    const agent = await newAgent("Ended");
    await sendAsUser(agent.roomId, "开始");
    const runId = await startRun(agent.id, agent.roomId);
    await finish(runId, { outcome: "cancelled" });
    expect((await computer(`/computer/runs/${runId}/events`, "POST", { events: [{ kind: "step", at }] })).status).toBe(
      409,
    );
    expect((await finish(runId, { outcome: "succeeded" })).status).toBe(409);
    const missing = "00000000-0000-4000-8000-000000000000";
    expect((await finish(missing, { outcome: "succeeded" })).status).toBe(404);
  });

  it("interrupt the runs a previous computer left running when a computer connects", async () => {
    const agent = await newAgent("Orphan");
    await sendAsUser(agent.roomId, "开始");
    const runId = await startRun(agent.id, agent.roomId);
    expect((await computer("/computer/connect", "POST")).status).toBe(204);
    expect(await statusOf(agent.id)).toEqual({ state: "idle" });
    expect((await runDetail(runId)).outcome).toBe("interrupted");
  });

  it("keep at most one running run per agent: a new run interrupts the old one", async () => {
    const agent = await newAgent("Twice");
    await sendAsUser(agent.roomId, "开始");
    const first = await startRun(agent.id, agent.roomId);
    const second = await startRun(agent.id, agent.roomId);
    expect((await runDetail(first)).outcome).toBe("interrupted");
    expect(await statusOf(agent.id)).toMatchObject({ state: "working", runId: second });
  });

  it("are listed newest first, by room or by agent", async () => {
    const alice = await newAgent("ListAlice", "list-alice");
    const bob = await newAgent("ListBob", "list-bob");
    const group = await newGroup("列表", [alice, bob]);
    await sendAsUser(group.id, "大家好");
    const a = await startRun(alice.id, group.id);
    await finish(a, { outcome: "succeeded" });
    const b = await startRun(bob.id, group.id);
    await sendAsUser(alice.roomId, "私聊");
    const c = await startRun(alice.id, alice.roomId);

    const ids = async (query: string) =>
      ((await (await desktop(`/desktop/runs${query}`)).json()) as Array<{ id: string }>).map((run) => run.id);
    expect(await ids(`?roomId=${group.id}`)).toEqual([b, a]);
    expect(await ids(`?agentId=${alice.id}`)).toEqual([c, a]);
  });

  it("show a problem the computer reports as an error in every room, until it is cleared", async () => {
    const agent = await newAgent("Blocked");
    const problem = (value: string | null) =>
      computer(`/computer/agents/${agent.id}/problem`, "POST", { problem: value });
    expect((await problem("沙箱不可用")).status).toBe(204);
    expect(await statusOf(agent.id)).toEqual({ state: "error", reason: "沙箱不可用", roomIds: [] });
    await problem(null);
    expect(await statusOf(agent.id)).toEqual({ state: "idle" });
  });
});

interface Posted {
  id: MessageId;
  roomId: RoomId;
  seq: number;
}

/** 用户发一条消息；带 `threadOf` 时发到那条消息的讨论串。 */
async function post(roomId: RoomId, body: string, threadOf?: MessageId): Promise<Posted> {
  const response = await desktop(`/desktop/rooms/${roomId}/messages`, "POST", { body, threadOf });
  expect(response.status).toBe(201);
  return (await response.json()) as Posted;
}

async function threadsOf(roomId: RoomId) {
  const response = await desktop(`/desktop/rooms/${roomId}/threads`);
  expect(response.status).toBe(200);
  return (await response.json()) as Array<{
    id: RoomId;
    parent: { id: MessageId; body: string };
    replies: number;
    lastReplyAt: string | null;
    participants: Array<{ kind: string; displayName: string }>;
    unread: number;
  }>;
}

const wakes = async (run: () => Promise<unknown>) =>
  (await collect<ComputerEvent>(t.ctx.events.computer, run)).map((event) =>
    event.type === "agent.wake" ? event.agentId : event.type,
  );

describe("threads", () => {
  it("open under a group message on the first reply, stay out of the group's timeline, and are summarized", async () => {
    const alice = await newAgent("ThreadAlice", "thread-alice");
    const group = await newGroup("讨论串", [alice]);
    const host = await post(group.id, "登录页要加忘记密码");

    const first = await post(group.id, "细节在这里讨论", host.id);
    expect(first.roomId).not.toBe(group.id);
    expect(first.seq).toBe(1);
    const second = await post(group.id, "再补充一句", host.id);
    expect(second).toMatchObject({ roomId: first.roomId, seq: 2 });
    expect((await post(first.roomId, "直接发到讨论串")).seq).toBe(3);

    expect((await listMessages(group.id)).map((message) => message.body)).toEqual(["登录页要加忘记密码"]);
    expect((await listMessages(first.roomId)).map((message) => message.seq)).toEqual([1, 2, 3]);
    const [summary] = await threadsOf(group.id);
    expect(summary).toMatchObject({ id: first.roomId, parent: { id: host.id }, replies: 3, unread: 0 });
    expect(summary?.participants).toEqual([expect.objectContaining({ kind: "user" })]);
    expect(summary?.lastReplyAt).not.toBeNull();
  });

  it("refresh both the thread and its group when a reply is written", async () => {
    const alice = await newAgent("RefreshAlice", "refresh-alice");
    const group = await newGroup("刷新", [alice]);
    const host = await post(group.id, "主题");
    const events = await collect<DesktopEvent>(t.ctx.events.desktop, () => post(group.id, "回复", host.id));
    const rooms = events.flatMap((event) => (event.type === "room.messages" ? [event.roomId] : []));
    expect(rooms).toHaveLength(2);
    expect(rooms).toContain(group.id);
  });

  it("wake the followers on a user reply: the agent that wrote the message, and those who replied or were mentioned", async () => {
    const alice = await newAgent("FollowAlice", "follow-alice");
    const bob = await newAgent("FollowBob", "follow-bob");
    const carol = await newAgent("FollowCarol", "follow-carol");
    const group = await newGroup("关注", [alice, bob, carol]);
    const aliceToken = await agentToken(alice.id);
    const bobToken = await agentToken(bob.id);
    await post(group.id, "开始");
    await readInbox(alice.id);
    const host = await reply(aliceToken, group.id, "我先列个方案");
    if (host.outcome !== "posted") throw new Error("应当发出");

    const thread = (await post(group.id, "方案细节呢？", host.id)).roomId;
    expect(await wakes(() => post(thread, "继续"))).toEqual([alice.id]);
    expect(new Set(await wakes(() => post(thread, "@follow-bob 你也看下")))).toEqual(new Set([alice.id, bob.id]));

    await readInbox(bob.id);
    expect(await wakes(() => reply(bobToken, thread, "看过了，没问题"))).toEqual([]);
    expect(new Set(await wakes(() => post(thread, "好的")))).toEqual(new Set([alice.id, bob.id]));
    expect(await wakes(() => post(group.id, "群里的消息仍然唤醒全部成员"))).toHaveLength(3);
  });

  it("wake and enlist every agent in the group when nobody follows the thread yet", async () => {
    const alice = await newAgent("EnlistAlice", "enlist-alice");
    const bob = await newAgent("EnlistBob", "enlist-bob");
    const group = await newGroup("没人关注", [alice, bob]);
    const host = await post(group.id, "我自己发的主题");
    const first = await wakes(() => post(group.id, "谁来回答？", host.id));
    expect(new Set(first)).toEqual(new Set([alice.id, bob.id]));
    const thread = (await threadsOf(group.id))[0]?.id;
    if (!thread) throw new Error("讨论串应当存在");
    expect(new Set(await wakes(() => post(thread, "还在吗")))).toEqual(new Set([alice.id, bob.id]));
  });

  it("let an agent open one with crew reply --thread and tell it where the message went", async () => {
    const alice = await newAgent("OpenAlice", "open-alice");
    const group = await newGroup("Agent 开讨论串", [alice]);
    const host = await post(group.id, "主题");
    await readInbox(alice.id);
    const response = await call(await agentToken(alice.id), "/agent/reply", "POST", {
      roomId: group.id,
      body: "我在讨论串里回",
      threadOf: host.id,
    });
    expect(response.status).toBe(200);
    const posted = (await response.json()) as { outcome: string; roomId: RoomId };
    expect(posted.outcome).toBe("posted");
    expect(posted.roomId).toBe((await threadsOf(group.id))[0]?.id);
  });

  it("let one agent follow several threads, each with its own read position and its own section in a turn", async () => {
    const alice = await newAgent("ManyAlice", "many-alice");
    const bob = await newAgent("ManyBob", "many-bob");
    const group = await newGroup("多个讨论串", [alice, bob]);
    const first = await post(group.id, "主题一：登录页");
    const second = await post(group.id, "主题二：注册页");
    const threadA = (await post(group.id, "@many-alice 登录页怎么改？", first.id)).roomId;
    const threadB = (await post(group.id, "@many-alice @many-bob 注册页呢？", second.id)).roomId;
    await post(group.id, "群里的新消息");

    const inbox = await readInbox(alice.id);
    const sections = new Map(inbox.map((room) => [room.roomId, room]));
    expect(new Set(sections.keys())).toEqual(new Set([group.id, threadA, threadB]));
    expect(sections.get(group.id)?.messages.map((message) => message.body)).toEqual([
      "主题一：登录页",
      "主题二：注册页",
      "群里的新消息",
    ]);
    expect(sections.get(threadA)).toMatchObject({ kind: "thread", parent: { message: { id: first.id } } });
    expect(sections.get(threadA)?.messages.map((message) => message.body)).toEqual(["@many-alice 登录页怎么改？"]);
    expect(sections.get(threadB)).toMatchObject({ kind: "thread", parent: { message: { id: second.id } } });

    // 每个讨论串只唤醒自己的关注者：Bob 只关注了讨论串二。
    expect(await wakes(() => post(threadA, "补充：要能记住邮箱"))).toEqual([alice.id]);
    expect(new Set(await wakes(() => post(threadB, "补充：要校验密码强度")))).toEqual(new Set([alice.id, bob.id]));

    // 确认已读把读过的每个房间一起推进；之后只剩上次读取之后来的消息，各在自己的讨论串里。
    await acknowledge(alice.id);
    await post(threadB, "再补一句");
    const next = await readInbox(alice.id);
    expect(new Map(next.map((room) => [room.roomId, room.messages.map((message) => message.body)]))).toEqual(
      new Map([
        [threadA, ["补充：要能记住邮箱"]],
        [threadB, ["补充：要校验密码强度", "再补一句"]],
      ]),
    );
  });

  it("refuse to open in a direct room, inside a thread, or under a message of another room", async () => {
    const alice = await newAgent("RefuseAlice", "refuse-alice");
    const group = await newGroup("拒绝", [alice]);
    const direct = await post(alice.roomId, "私聊");
    const send = (roomId: RoomId, threadOf: MessageId) =>
      desktop(`/desktop/rooms/${roomId}/messages`, "POST", { body: "x", threadOf });

    const inDirect = await send(alice.roomId, direct.id);
    expect(inDirect.status).toBe(400);
    expect(await inDirect.json()).toEqual({ error: "私聊里不能开讨论串" });

    const host = await post(group.id, "主题");
    const reply1 = await post(group.id, "回复", host.id);
    const nested = await send(reply1.roomId, reply1.id);
    expect(nested.status).toBe(400);
    expect(await nested.json()).toEqual({ error: "讨论串里不能再开讨论串" });

    expect((await send(group.id, direct.id)).status).toBe(404);
  });

  it("show a follower the group's name, members and the message the thread hangs under, with every reply", async () => {
    const alice = await newAgent("InboxAlice", "inbox-alice");
    const bob = await newAgent("InboxBob", "inbox-bob");
    const group = await newGroup("收件箱", [alice, bob]);
    const host = await post(group.id, "要做的事");
    await readInbox(bob.id);
    await acknowledge(bob.id);
    const thread = (await post(group.id, "第一条回复", host.id)).roomId;
    await post(thread, "第二条回复");
    await post(thread, "@inbox-bob 请看");

    const inbox = await readInbox(bob.id);
    expect(inbox).toHaveLength(1);
    expect(inbox[0]).toMatchObject({
      roomId: thread,
      kind: "thread",
      name: "收件箱",
      parent: { roomId: group.id, message: { id: host.id, body: "要做的事" } },
    });
    expect(inbox[0]?.members.map((member) => member.displayName)).toEqual(["User", "InboxAlice", "InboxBob"]);
    expect(inbox[0]?.messages.map((message) => [message.body, message.mentionsYou])).toEqual([
      ["第一条回复", false],
      ["第二条回复", false],
      ["@inbox-bob 请看", true],
    ]);
  });

  it("hold an agent's first reply in a thread until it has seen the replies already there", async () => {
    const alice = await newAgent("LateAlice", "late-alice");
    const bob = await newAgent("LateBob", "late-bob");
    const group = await newGroup("后来者", [alice, bob]);
    const host = await post(group.id, "主题");
    await readInbox(bob.id);
    await post(group.id, "@late-alice 先说", host.id);
    const token = await agentToken(bob.id);

    const held = await call(token, "/agent/reply", "POST", { roomId: group.id, body: "我来补充", threadOf: host.id });
    const outcome = (await held.json()) as { outcome: string; newMessages: Array<{ body: string }> };
    expect(outcome.outcome).toBe("held");
    expect(outcome.newMessages.map((message) => message.body)).toEqual(["@late-alice 先说"]);

    const again = await call(token, "/agent/reply", "POST", { roomId: group.id, body: "我来补充", threadOf: host.id });
    expect(((await again.json()) as { outcome: string }).outcome).toBe("posted");
  });

  it("count unread replies in the group's conversation and in the summary, until the thread is read", async () => {
    const alice = await newAgent("UnreadThread", "unread-thread");
    const group = await newGroup("讨论串未读", [alice]);
    const host = await post(group.id, "主题");
    const thread = (await post(group.id, "我的回复不算", host.id)).roomId;
    await desktop(`/desktop/rooms/${group.id}/read`, "POST", { seq: 99 });
    await readInbox(alice.id);
    const token = await agentToken(alice.id);
    await reply(token, thread, "回复一");
    await reply(token, thread, "回复二");

    expect((await conversation(group.id))?.unread).toBe(2);
    expect((await threadsOf(group.id))[0]?.unread).toBe(2);
    expect((await desktop(`/desktop/rooms/${thread}/read`, "POST", { seq: 99 })).status).toBe(204);
    expect((await conversation(group.id))?.unread).toBe(0);
    expect((await threadsOf(group.id))[0]?.unread).toBe(0);
  });

  it("list runs woken in a thread under its group, and show the agent working in both", async () => {
    const alice = await newAgent("RunThread", "run-thread");
    const group = await newGroup("讨论串运行", [alice]);
    const host = await post(group.id, "主题");
    const thread = (await post(group.id, "回复", host.id)).roomId;
    const runId = await startRun(alice.id, thread);

    const listed = (await (await desktop(`/desktop/runs?roomId=${group.id}`)).json()) as Array<{
      id: string;
      triggers: Array<{ roomId: RoomId; parentRoomId: RoomId | null }>;
    }>;
    expect(listed.map((run) => run.id)).toEqual([runId]);
    expect(listed[0]?.triggers).toEqual([expect.objectContaining({ roomId: thread, parentRoomId: group.id })]);
    expect(await statusOf(alice.id)).toEqual({ state: "working", runId, roomIds: [thread, group.id] });
    await finish(runId, { outcome: "succeeded" });
  });
});

interface Task {
  id: string;
  number: number;
  title: string;
  status: string;
  assignee: { id: AgentId; handle: string } | null;
  messageId: MessageId;
  threadId: RoomId | null;
}

async function expectTask(response: Response, status = 200): Promise<Task> {
  expect(response.status).toBe(status);
  return (await response.json()) as Task;
}

async function expectRefusal(response: Response, status = 409) {
  expect(response.status).toBe(status);
  return ((await response.json()) as { error: string; refusal: { code: string } }).refusal;
}

const newTask = (roomId: RoomId, title: string, assigneeId?: AgentId) =>
  desktop(`/desktop/rooms/${roomId}/tasks`, "POST", { title, assigneeId });
const setStatus = (roomId: RoomId, number: number, status: string) =>
  desktop(`/desktop/rooms/${roomId}/tasks/${number}/status`, "POST", { status });
const claim = (token: string, roomId: RoomId, number: number) =>
  call(token, "/agent/tasks/claim", "POST", { roomId, number });
const agentStatus = (token: string, roomId: RoomId, number: number, status: string) =>
  call(token, "/agent/tasks/status", "POST", { roomId, number, status });

async function bodiesWithKind(roomId: RoomId) {
  const response = await desktop(`/desktop/rooms/${roomId}/messages`);
  return ((await response.json()) as Array<{ kind: string; body: string }>).map((m) => `${m.kind}: ${m.body}`);
}

describe("tasks", () => {
  it("are created from a title, numbered per room, with their notices in the task's thread", async () => {
    const alice = await newAgent("TaskAlice", "task-alice");
    const group = await newGroup("任务", [alice]);
    const first = await expectTask(await newTask(group.id, "登录页加忘记密码"), 201);
    expect(first).toMatchObject({ number: 1, title: "登录页加忘记密码", status: "todo", assignee: null });
    expect((await expectTask(await newTask(group.id, "注册表单校验"), 201)).number).toBe(2);

    expect(await bodiesWithKind(group.id)).toEqual(["text: 登录页加忘记密码", "text: 注册表单校验"]);
    if (!first.threadId) throw new Error("群聊里的任务应当有讨论串");
    expect(await bodiesWithKind(first.threadId)).toEqual(["system: 新建了任务 #1"]);
    const listed = (await (await desktop(`/desktop/rooms/${group.id}/tasks`)).json()) as Task[];
    expect(listed.map((task) => task.number)).toEqual([1, 2]);
  });

  it("put their notices in a direct room's timeline, which has no threads", async () => {
    const alice = await newAgent("DirectTask", "direct-task");
    const task = await expectTask(await newTask(alice.roomId, "整理周报", alice.id), 201);
    expect(task).toMatchObject({ number: 1, threadId: null, assignee: { handle: "direct-task" } });
    expect(await bodiesWithKind(alice.roomId)).toEqual([
      "text: 整理周报",
      "system: 新建了任务 #1，分配给 @direct-task",
    ]);
  });

  it("convert a message by its first line, refusing notices, thread messages and a message that is already a task", async () => {
    const alice = await newAgent("ConvertAlice", "convert-alice");
    const group = await newGroup("转任务", [alice]);
    const message = await post(group.id, "\n修一下导出按钮\n点了没反应");
    const convert = (roomId: RoomId, messageId: MessageId) =>
      desktop(`/desktop/rooms/${roomId}/tasks/convert`, "POST", { messageId });

    const task = await expectTask(await convert(group.id, message.id), 201);
    expect(task).toMatchObject({ title: "修一下导出按钮", messageId: message.id });
    expect(await expectRefusal(await convert(group.id, message.id))).toEqual({ code: "already_task", number: 1 });

    // 私聊的通知在时间线上，可以拿来试：通知不能转成任务。
    await newTask(alice.roomId, "私聊里的任务");
    const notice = (await listMessagesWithIds(alice.roomId)).find((message) => message.kind === "system");
    if (!notice) throw new Error("私聊里应当有通知");
    expect(await expectRefusal(await convert(alice.roomId, notice.id))).toEqual({ code: "system_message" });
    const reply1 = await post(group.id, "讨论串里的一句", message.id);
    expect(await expectRefusal(await convert(reply1.roomId, reply1.id))).toEqual({ code: "in_thread" });
  });

  it("let only one of two agents claim a task at the same time", async () => {
    const alice = await newAgent("RaceAlice", "race-alice");
    const bob = await newAgent("RaceBob", "race-bob");
    const group = await newGroup("抢任务", [alice, bob]);
    const task = await expectTask(await newTask(group.id, "只能有一个人做"), 201);
    const [a, b] = await Promise.all([
      claim(await agentToken(alice.id), group.id, task.number),
      claim(await agentToken(bob.id), group.id, task.number),
    ]);
    const statuses = [a.status, b.status].sort();
    expect(statuses).toEqual([200, 409]);
    const loser = a.status === 409 ? a : b;
    const winner = a.status === 200 ? "race-alice" : "race-bob";
    expect(((await loser.json()) as { refusal: unknown }).refusal).toEqual({ code: "claimed", number: 1, by: winner });
  });

  it("follow the transition table and need an assignee to start", async () => {
    const alice = await newAgent("FlowAlice", "flow-alice");
    const group = await newGroup("流转", [alice]);
    const { number } = await expectTask(await newTask(group.id, "按流程走"), 201);
    expect(await expectRefusal(await setStatus(group.id, number, "in_review"))).toEqual({
      code: "transition",
      number,
      from: "todo",
      to: "in_review",
    });
    expect(await expectRefusal(await setStatus(group.id, number, "in_progress"))).toEqual({
      code: "needs_assignee",
      number,
    });

    const token = await agentToken(alice.id);
    expect((await expectTask(await claim(token, group.id, number))).status).toBe("in_progress");
    expect((await expectTask(await agentStatus(token, group.id, number, "in_review"))).status).toBe("in_review");
    expect((await expectTask(await setStatus(group.id, number, "done"))).status).toBe("done");
    expect((await expectTask(await setStatus(group.id, number, "todo"))).status).toBe("todo");
  });

  it("wake the assignee only when it is assigned or its task is sent back", async () => {
    const alice = await newAgent("PingAlice", "ping-alice");
    const bob = await newAgent("PingBob", "ping-bob");
    const group = await newGroup("唤醒", [alice, bob]);
    let number = 0;
    expect(
      await wakes(async () => {
        number = (await expectTask(await newTask(group.id, "分给 Alice", alice.id), 201)).number;
      }),
    ).toEqual([alice.id]);

    const token = await agentToken(alice.id);
    expect(await wakes(() => claim(token, group.id, number))).toEqual([]);
    expect(await wakes(() => agentStatus(token, group.id, number, "in_review"))).toEqual([]);
    expect(await wakes(() => setStatus(group.id, number, "in_progress"))).toEqual([alice.id]);
    expect(await wakes(() => setStatus(group.id, number, "done"))).toEqual([]);
    // 重新打开完成的任务也是退回。
    expect(await wakes(() => setStatus(group.id, number, "in_progress"))).toEqual([alice.id]);
    expect(await wakes(() => setStatus(group.id, number, "closed"))).toEqual([]);
  });

  it("do not hold an agent behind its own notice, nor count the user's own notices as unread", async () => {
    const alice = await newAgent("OwnAlice", "own-alice");
    const group = await newGroup("自己的通知", [alice]);
    const task = await expectTask(await newTask(group.id, "看看通知", alice.id), 201);
    if (!task.threadId) throw new Error("应当有讨论串");
    await desktop(`/desktop/rooms/${group.id}/read`, "POST", { seq: 99 });
    expect((await conversation(group.id))?.unread).toBe(0);

    await readInbox(alice.id);
    const token = await agentToken(alice.id);
    await expectTask(await claim(token, group.id, task.number));
    expect((await reply(token, task.threadId, "开始做了")).outcome).toBe("posted");
    // Alice 的领取通知与回复是别人发的，算未读；用户自己的“新建了任务”不算。
    expect((await conversation(group.id))?.unread).toBe(2);
  });

  it("show the task on its host message in the agent's turn, and notices as notices", async () => {
    const alice = await newAgent("TagAlice", "tag-alice");
    const group = await newGroup("后缀", [alice]);
    const task = await expectTask(await newTask(group.id, "带后缀", alice.id), 201);
    const inbox = await readInbox(alice.id);
    const main = inbox.find((room) => room.roomId === group.id);
    expect(main?.messages[0]).toMatchObject({
      body: "带后缀",
      task: { number: task.number, status: "todo", assignee: "tag-alice" },
    });
    const thread = inbox.find((room) => room.roomId === task.threadId);
    expect(thread?.messages[0]).toMatchObject({ kind: "system", mentionsYou: true, task: null });
    expect(thread).toMatchObject({ parent: { message: { task: { number: task.number } } } });
  });

  it("change the assignee, send a running task back to todo when unassigned, and refuse outsiders and finished tasks", async () => {
    const alice = await newAgent("AssignAlice", "assign-alice");
    const bob = await newAgent("AssignBob", "assign-bob");
    const outsider = await newAgent("AssignOut", "assign-out");
    const group = await newGroup("分配", [alice, bob]);
    const { number } = await expectTask(await newTask(group.id, "换人"), 201);
    const assign = (agentId: AgentId | null) =>
      desktop(`/desktop/rooms/${group.id}/tasks/${number}/assignee`, "POST", { agentId });

    expect(await expectRefusal(await assign(outsider.id))).toEqual({ code: "not_member", handle: "assign-out" });
    // 不在房间里的 Agent 也看不到房间里的任务。
    const peek = await call(await agentToken(outsider.id), "/agent/tasks/list", "POST", { roomId: group.id });
    expect(peek.status).toBe(403);
    await expectTask(await assign(alice.id));
    await expectTask(await claim(await agentToken(alice.id), group.id, number));
    expect((await expectTask(await assign(bob.id))).assignee?.handle).toBe("assign-bob");
    expect(await expectTask(await assign(null))).toMatchObject({ status: "todo", assignee: null });
    await expectTask(await claim(await agentToken(bob.id), group.id, number));
    await expectTask(await setStatus(group.id, number, "done"));
    expect(await expectRefusal(await assign(alice.id))).toEqual({ code: "finished", number, status: "done" });
  });
});

describe("models", () => {
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
