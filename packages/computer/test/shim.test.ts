import { ComputerAgent, MESSAGE_BODY_MAX, RoomId } from "@crew/protocol";
import { createTestApp, TEST_COMPUTER_TOKEN, TEST_DESKTOP_TOKEN, type TestApp } from "@crew/server/testing";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { ServerClient } from "../src/client";
import { type CliIo, runCli } from "../src/shim/cli";

// `crew` 命令：真实的 Server 应用（内存中，临时数据库），stdin、环境变量与凭证文件由测试提供。

const SERVER_URL = "http://127.0.0.1:1";
const TOKEN_FILE = "/crew/runtime/agents/alice/token";
const MISSING_ROOM = RoomId.parse("00000000-0000-4000-8000-000000000000");

let t: TestApp;
let alice: ComputerAgent;
let bob: ComputerAgent;
let aliceToken: string;

beforeEach(async () => {
  t = await createTestApp();
  alice = await newAgent("Alice");
  bob = await newAgent("Bob");
  aliceToken = await new ServerClient(SERVER_URL, TEST_COMPUTER_TOKEN, t.fetch).issueAgentToken(alice.id);
});

afterEach(async () => {
  await t.close();
});

async function newAgent(displayName: string): Promise<ComputerAgent> {
  const response = await t.request("/desktop/agents", {
    method: "POST",
    headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}`, "Content-Type": "application/json" },
    body: JSON.stringify({ displayName, handle: displayName.toLowerCase(), persona: "同事", model: "fake/model" }),
  });
  return ComputerAgent.parse(await response.json());
}

/** 以用户的身份发一条消息，返回它的 ID。 */
async function sendAsUser(roomId: RoomId, body: string): Promise<string> {
  const response = await t.request(`/desktop/rooms/${roomId}/messages`, {
    method: "POST",
    headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}`, "Content-Type": "application/json" },
    body: JSON.stringify({ body }),
  });
  return ((await response.json()) as { id: string }).id;
}

/** 新建一个有 Alice 与 Bob 的群聊，返回它的 ID。 */
async function newGroup(name: string): Promise<RoomId> {
  const response = await t.request("/desktop/groups", {
    method: "POST",
    headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}`, "Content-Type": "application/json" },
    body: JSON.stringify({ name, agentIds: [alice.id, bob.id] }),
  });
  return RoomId.parse(((await response.json()) as { id: string }).id);
}

interface Run {
  stdin?: string;
  env?: Record<string, string | undefined>;
  token?: string;
  fetch?: typeof fetch;
}

/** 以 Alice 的身份运行一次 `crew`。 */
async function crew(args: string[], run: Run = {}) {
  let stdout = "";
  let stderr = "";
  const io: CliIo = {
    env: run.env ?? { CREW_SERVER_URL: SERVER_URL, CREW_TOKEN_FILE: TOKEN_FILE },
    readStdin: async () => run.stdin ?? "",
    readFile: async (path) => {
      if (path !== TOKEN_FILE) throw new Error(`ENOENT: ${path}`);
      return `${run.token ?? aliceToken}\n`;
    },
    stdout: (text) => {
      stdout += text;
    },
    stderr: (text) => {
      stderr += text;
    },
    fetch: run.fetch ?? t.fetch,
  };
  const code = await runCli(args, io);
  return { code, stdout, stderr };
}

async function bodies(roomId: RoomId): Promise<string[]> {
  const response = await t.request(`/desktop/rooms/${roomId}/messages`, {
    headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}` },
  });
  const messages = (await response.json()) as Array<{ body: string }>;
  return messages.map((message) => message.body);
}

describe("crew reply", () => {
  it("posts the message from standard input exactly as written, without the heredoc's final newline", async () => {
    const body = 'Run `ls $HOME` and check "quotes".\n\n  - indented item\n';
    const result = await crew(["reply", alice.roomId], { stdin: body });

    expect(result).toEqual({ code: 0, stdout: `Message sent to room ${alice.roomId}.\n`, stderr: "" });
    expect(await bodies(alice.roomId)).toEqual(['Run `ls $HOME` and check "quotes".\n\n  - indented item']);
  });

  it("refuses a message given as an argument", async () => {
    const result = await crew(["reply", alice.roomId, "hello"]);
    expect(result.code).toBe(1);
    expect(await bodies(alice.roomId)).toEqual([]);
  });

  it.each([
    ["an empty message", { stdin: " \n\n" }],
    ["a message over the limit", { stdin: "a".repeat(MESSAGE_BODY_MAX + 1) }],
    ["no server address", { stdin: "hi", env: { CREW_TOKEN_FILE: TOKEN_FILE } }],
    ["no token file", { stdin: "hi", env: { CREW_SERVER_URL: SERVER_URL, CREW_TOKEN_FILE: "/elsewhere" } }],
    ["a token the server does not know", { stdin: "hi", token: "stale" }],
  ])("fails with exit code 1 and posts nothing for %s", async (_name, run: Run) => {
    const result = await crew(["reply", alice.roomId], run);
    expect(result.code).toBe(1);
    expect(result.stdout).toBe("");
    expect(result.stderr).toMatch(/^error: /);
    expect(await bodies(alice.roomId)).toEqual([]);
  });

  it("does not post while the person has written something Alice has not seen, and shows it instead", async () => {
    await sendAsUser(alice.roomId, "等等，还有一件事");

    const held = await crew(["reply", alice.roomId], { stdin: "好的" });
    expect(held.code).toBe(1);
    expect(held.stdout).toContain("Not sent: 1 new message");
    expect(held.stdout).toContain("User (user): 等等，还有一件事");
    expect(await bodies(alice.roomId)).toEqual(["等等，还有一件事"]);

    // 新消息已经给 Alice 看过，再发一次就能发出。
    const retried = await crew(["reply", alice.roomId], { stdin: "好的，我一起看" });
    expect(retried.code).toBe(0);
    expect(await bodies(alice.roomId)).toEqual(["等等，还有一件事", "好的，我一起看"]);
  });

  it("posts in the thread under a message with --thread, starting it the first time", async () => {
    const group = await newGroup("发版");
    const host = await sendAsUser(group, "回归测试的范围定一下");
    await new ServerClient(SERVER_URL, TEST_COMPUTER_TOKEN, t.fetch).readInbox(alice.id);

    const first = await crew(["reply", group, "--thread", host], { stdin: "我来列一下" });
    expect(first.code).toBe(0);
    const threadId = first.stdout.match(/thread (\S+), under/)?.[1];
    expect(threadId).toBeDefined();
    const second = await crew(["reply", group, "--thread", host], { stdin: "补充一项" });
    expect(second.stdout).toContain(`thread ${threadId},`);
    expect(await bodies(group)).toEqual(["回归测试的范围定一下"]);
    expect(await bodies(RoomId.parse(threadId))).toEqual(["我来列一下", "补充一项"]);
  });

  it("creates, claims and moves a task, and refuses a second claim with the holder's handle", async () => {
    const group = await newGroup("任务");
    const created = await crew(["task", "create", group, "写发布说明", "--assign", "bob"]);
    expect(created.code).toBe(0);
    expect(created.stdout).toContain('task #1 "写发布说明" (todo, assigned to @bob)');

    const taken = await crew(["task", "claim", group, "#1"]);
    expect(taken).toMatchObject({
      code: 1,
      stderr: "error: task #1 is already taken by @bob. Don't start work on it.\n",
    });

    expect((await crew(["task", "create", group, "检查崩溃报告"])).code).toBe(0);
    const claimed = await crew(["task", "claim", group, "2"]);
    expect(claimed.stdout).toContain('task #2 "检查崩溃报告" (in_progress, assigned to @alice)');
    expect((await crew(["task", "status", group, "2", "in_review"])).stdout).toContain(
      "(in_review, assigned to @alice)",
    );

    // --note 真的发到了 Server：从任务的讨论串读回最后一条通知。
    expect((await crew(["task", "status", group, "2", "in_progress", "--note", " Shorten the title "])).code).toBe(0);
    const headers = { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}` };
    const tasks = (await (await t.request(`/desktop/rooms/${group}/tasks`, { headers })).json()) as Array<{
      number: number;
      threadId: string;
    }>;
    const threadId = tasks.find((task) => task.number === 2)?.threadId;
    const thread = (await (await t.request(`/desktop/rooms/${threadId}/messages`, { headers })).json()) as Array<{
      body: string;
      notice: unknown;
    }>;
    expect(thread.at(-1)).toMatchObject({
      body: "把 #2 从待审改成进行中：Shorten the title",
      notice: { type: "task.status", from: "in_review", to: "in_progress", note: "Shorten the title" },
    });
  });

  it("sets a reminder relative to now or every day, lists it by when it fires, and cancels it", async () => {
    const before = Date.now();
    const soon = await crew(["remind", alice.roomId, "Check CI", "--in", "30m"]);
    expect(soon.code).toBe(0);
    expect(soon.stdout).toContain('Reminder set: "Check CI" at ');
    expect((await crew(["remind", alice.roomId, "Morning summary", "--daily", "9:00"])).stdout).toContain(
      "(daily at 09:00)",
    );

    const list = await crew(["remind", "list"]);
    const lines = list.stdout.split("\n").filter((line) => line.startsWith("  "));
    expect(lines).toHaveLength(2);
    expect(lines[0]).toContain('"Check CI"');
    const when = new Date(/at (\S+?),/.exec(lines[0] ?? "")?.[1] ?? "").getTime();
    expect(when).toBeGreaterThanOrEqual(before + 30 * 60_000 - 1000);
    expect(when).toBeLessThanOrEqual(Date.now() + 30 * 60_000 + 1000);

    const id = lines[0]?.trim().split(/\s+/)[0] ?? "";
    expect((await crew(["remind", "cancel", id])).code).toBe(0);
    expect((await crew(["remind", "list"])).stdout).not.toContain("Check CI");

    // 带时区的 ISO 8601（带秒或不带、Z 或偏移）照常接受；范围检查只拒绝日历上没有的时刻。
    const tomorrow = new Date(Date.now() + 24 * 60 * 60_000);
    const iso = tomorrow.toISOString().slice(0, 16);
    const pad = (n: number) => String(n).padStart(2, "0");
    const local = `${tomorrow.getFullYear()}-${pad(tomorrow.getMonth() + 1)}-${pad(tomorrow.getDate())} 09:00`;
    const spaced = `${iso.replace("T", " ")}+00:00`;
    for (const at of [`${iso}:00Z`, `${iso}+00:00`, `${iso.toLowerCase()}z`, spaced, local]) {
      expect((await crew(["remind", alice.roomId, "Valid time", "--at", at])).code).toBe(0);
    }
  });

  it("does not post in a room the agent is not a member of", async () => {
    const result = await crew(["reply", bob.roomId], { stdin: "hi" });
    expect(result.code).toBe(1);
    expect(await bodies(bob.roomId)).toEqual([]);
  });
});

describe("crew output", () => {
  // 模型读到的全部文字，逐字锁定在一个文件里。
  it("matches the reviewed text", async () => {
    const unreachable: typeof fetch = async () => {
      throw new TypeError("fetch failed");
    };
    const timedOut: typeof fetch = async () => {
      throw new DOMException("The operation was aborted due to timeout", "TimeoutError");
    };
    const cases: Array<[string, string[], Run?]> = [
      ["crew --help", ["--help"]],
      ["crew", []],
      ["crew reply --help", ["reply", "--help"]],
      ["sent", ["reply", alice.roomId], { stdin: "hello\n" }],
      ["no room id", ["reply"]],
      ["message as an argument", ["reply", alice.roomId, "hello"]],
      ["unknown command", ["send"]],
      ["not a room id", ["reply", "general"], { stdin: "hello" }],
      ["empty message", ["reply", alice.roomId], { stdin: "\n" }],
      ["message over the limit", ["reply", alice.roomId], { stdin: "a".repeat(MESSAGE_BODY_MAX + 1) }],
      ["outside a turn", ["reply", alice.roomId], { stdin: "hello", env: {} }],
      ["unknown token", ["reply", alice.roomId], { stdin: "hello", token: "stale" }],
      ["not a member", ["reply", bob.roomId], { stdin: "hello" }],
      ["no such room", ["reply", MISSING_ROOM], { stdin: "hello" }],
      ["server unreachable", ["reply", alice.roomId], { stdin: "hello", fetch: unreachable }],
      ["server timed out", ["reply", alice.roomId], { stdin: "hello", fetch: timedOut }],
    ];

    const sections: string[] = [];
    const record = async (title: string, args: string[], run?: Run) => {
      const result = await crew(args, run);
      const output = [result.stdout && `stdout:\n${result.stdout}`, result.stderr && `stderr:\n${result.stderr}`]
        .filter(Boolean)
        .join("\n");
      sections.push(`## ${title} (exit ${result.code})\n\n\`\`\`text\n${output.trimEnd()}\n\`\`\`\n`);
    };
    for (const [title, args, run] of cases) await record(title, args, run);
    // 讨论串：在群聊的一条消息下开，私聊、讨论串里与别的房间的消息都不行。
    const group = await newGroup("Release");
    const host = await sendAsUser(group, "Scope of the regression run?");
    const directMessage = await sendAsUser(alice.roomId, "A direct message.");
    await new ServerClient(SERVER_URL, TEST_COMPUTER_TOKEN, t.fetch).readInbox(alice.id);
    const posted = await crew(["reply", group, "--thread", host], { stdin: "I'll list it." });
    const threadId = RoomId.parse(posted.stdout.match(/thread (\S+), under/)?.[1]);
    const ids = (text: string) =>
      text
        .replaceAll(group, "<group-room>")
        .replaceAll(threadId, "<thread>")
        .replaceAll(host, "<host-message>")
        .replaceAll(directMessage, "<direct-message>");
    sections.push(
      `## sent to a thread (exit ${posted.code})\n\n\`\`\`text\nstdout:\n${ids(posted.stdout).trimEnd()}\n\`\`\`\n`,
    );
    const threadCases: Array<[string, string[], Run?]> = [
      ["not a message id", ["reply", group, "--thread", "first"], { stdin: "hello" }],
      ["thread in a direct room", ["reply", alice.roomId, "--thread", directMessage], { stdin: "hello" }],
      ["thread in a thread", ["reply", threadId, "--thread", host], { stdin: "hello" }],
      ["message from another room", ["reply", group, "--thread", directMessage], { stdin: "hello" }],
    ];
    for (const [title, args, run] of threadCases) {
      const before = sections.length;
      await record(title, args, run);
      sections[before] = ids(sections[before] ?? "");
    }
    // 任务：在同一个群里新建、转换、领取、改状态、分配，以及各种拒绝。讨论串的 ID 每次运行都不同，按出现顺序编号。
    const threadIds = new Map<string, string>();
    const taskIds = (text: string) =>
      ids(text).replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g, (id) => {
        if (!threadIds.has(id)) threadIds.set(id, `<task-thread-${threadIds.size + 1}>`);
        return threadIds.get(id) ?? id;
      });
    const taskCases: Array<[string, string[]]> = [
      ["crew task --help", ["task", "--help"]],
      ["task list, empty", ["task", "list", group]],
      ["task create", ["task", "create", group, "Write the release notes"]],
      ["task create, assigned", ["task", "create", group, "Check the crash reports", "--assign", "@bob"]],
      ["task convert", ["task", "convert", group, host]],
      ["task convert, message of another room", ["task", "convert", group, directMessage]],
      ["task list", ["task", "list", group]],
      ["task claim", ["task", "claim", group, "1"]],
      ["task claim, taken", ["task", "claim", group, "2"]],
      ["task claim, no such task", ["task", "claim", group, "99"]],
      ["task status", ["task", "status", group, "1", "in_review"]],
      ["task status, not allowed", ["task", "status", group, "2", "in_review"]],
      ["task status, unknown status", ["task", "status", group, "1", "finished"]],
      ["task status, with a note", ["task", "status", group, "1", "in_progress", "--note", "Shorten the title"]],
      ["task status, empty note", ["task", "status", group, "1", "in_review", "--note", "  "]],
      ["task status, unchanged with a note", ["task", "status", group, "1", "in_progress", "--note", "Again"]],
      ["task assign, not in the room", ["task", "assign", group, "3", "nobody"]],
    ];
    for (const [title, args] of taskCases) {
      const before = sections.length;
      await record(title, args);
      sections[before] = taskIds(sections[before] ?? "");
    }
    // 提醒：绝对时间的设定、列出、取消与各种拒绝。时区偏移与提醒 ID 每台机器、每次运行都不同，换成占位符。
    // 日期取 30 天以后（写死的日期过了就成了“已经过去”），输出里换成 <date>。
    const later = new Date(Date.now() + 30 * 24 * 60 * 60_000);
    const laterDay = [later.getFullYear(), later.getMonth() + 1, later.getDate()]
      .map((part) => String(part).padStart(2, "0"))
      .join("-");
    const reminderIds = new Map<string, string>();
    const reminderText = (text: string) =>
      text
        .replaceAll(alice.roomId, "<alice-room>")
        .replaceAll(laterDay, "<date>")
        .replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g, (id) => {
          if (!reminderIds.has(id)) reminderIds.set(id, `<reminder-${reminderIds.size + 1}>`);
          return reminderIds.get(id) ?? id;
        })
        .replace(/([+-]\d{2}:\d{2}|Z)\)/g, "<offset>)")
        .replace(/T(\d{2}:\d{2}:\d{2})[+-]\d{2}:\d{2}/g, "T$1<offset>");
    const reminderCases: Array<[string, string[]]> = [
      ["crew remind --help", ["remind", "--help"]],
      ["crew remind set --help", ["remind", "set", "--help"]],
      ["remind list, empty", ["remind", "list"]],
      ["remind at a date", ["remind", alice.roomId, "Send the weekly report", "--at", `${laterDay} 09:00`]],
      ["remind list", ["remind", "list"]],
      ["remind, no time", ["remind", alice.roomId, "Something"]],
      ["remind, two times", ["remind", alice.roomId, "Something", "--in", "5m", "--daily", "09:00"]],
      ["remind, bad duration", ["remind", alice.roomId, "Something", "--in", "soon"]],
      ["remind, in the past", ["remind", alice.roomId, "Something", "--at", "2020-01-01 09:00"]],
      ["remind, too often", ["remind", alice.roomId, "Something", "--every", "2m"]],
      ["remind, bad weekly", ["remind", alice.roomId, "Something", "--weekly", "someday@9"]],
      // 不存在的时刻不能被顺延成别的时间。
      ["remind, minute out of range", ["remind", alice.roomId, "Something", "--at", "18:99"]],
      ["remind, hour out of range", ["remind", alice.roomId, "Something", "--at", "25:00"]],
      ["remind, no such date", ["remind", alice.roomId, "Something", "--at", "2027-02-30 09:00"]],
      ["remind, no such date in ISO", ["remind", alice.roomId, "Something", "--at", "2027-02-30T09:00:00+08:00"]],
      ["remind, too far ahead", ["remind", alice.roomId, "Something", "--in", "999999999999m"]],
    ];
    for (const [title, args] of reminderCases) {
      const before = sections.length;
      await record(title, args);
      sections[before] = reminderText(sections[before] ?? "");
    }
    const [firstReminder] = reminderIds.keys();
    const cancelBefore = sections.length;
    await record("remind cancel", ["remind", "cancel", firstReminder ?? ""]);
    await record("remind cancel, again", ["remind", "cancel", firstReminder ?? ""]);
    sections[cancelBefore] = reminderText(sections[cancelBefore] ?? "");
    sections[cancelBefore + 1] = reminderText(sections[cancelBefore + 1] ?? "");
    // 静音：定时与一直静音、解除，以及私聊、讨论串与时长的拒绝。到期时间每次运行都不同，换成占位符。
    const muteText = (text: string) =>
      ids(text)
        .replaceAll(alice.roomId, "<alice-room>")
        .replace(/until \d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-]\d{2}:\d{2}/, "until <time>");
    const muteCases: Array<[string, string[]]> = [
      ["crew mute --help", ["mute", "--help"]],
      ["mute for a while", ["mute", group, "--for", "2h"]],
      ["mute until unmuted", ["mute", group]],
      ["unmute", ["unmute", group]],
      ["mute a direct room", ["mute", alice.roomId]],
      ["mute a thread", ["mute", threadId]],
      ["mute, too short", ["mute", group, "--for", "5m"]],
      ["mute, too long", ["mute", group, "--for", "8d"]],
    ];
    for (const [title, args] of muteCases) {
      const before = sections.length;
      await record(title, args);
      sections[before] = muteText(sections[before] ?? "");
    }
    // 最后：用户发了 Alice 还没看到的消息，回复被拦下。
    const unseen = await sendAsUser(alice.roomId, "Wait, one more thing:\ncheck the tests too.");
    await record("held", ["reply", alice.roomId], { stdin: "On it." });
    // 新消息多于一次能返回的条数：Server 先返回最早的一批，并说明后面还有几条。
    const heldWithMore: typeof fetch = async () =>
      Response.json({
        outcome: "held",
        roomId: alice.roomId,
        newMessages: [
          {
            id: unseen,
            seq: 2,
            kind: "text",
            notice: null,
            author: { kind: "user", id: "u", displayName: "User", handle: null },
            body: "First of many.",
            createdAt: "2026-10-05T10:00:00.000Z",
          },
        ],
        omitted: 2,
      });
    await record("held, more to come", ["reply", alice.roomId], { stdin: "On it.", fetch: heldWithMore });
    const text = sections
      .join("\n")
      .replaceAll(alice.roomId, "<alice-room>")
      .replaceAll(bob.roomId, "<bob-room>")
      .replaceAll(unseen, "<message-id>");
    await expect(`# crew output\n\n${text}`).toMatchFileSnapshot("./__snapshots__/shim-output.md");
  });
});
