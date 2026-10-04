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
  const response = await t.app.request("/desktop/agents", {
    method: "POST",
    headers: { Authorization: `Bearer ${TEST_DESKTOP_TOKEN}`, "Content-Type": "application/json" },
    body: JSON.stringify({ displayName, persona: "同事", model: "fake/model" }),
  });
  return ComputerAgent.parse(await response.json());
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
  const response = await t.app.request(`/desktop/rooms/${roomId}/messages`, {
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
    for (const [title, args, run] of cases) {
      const result = await crew(args, run);
      const output = [result.stdout && `stdout:\n${result.stdout}`, result.stderr && `stderr:\n${result.stderr}`]
        .filter(Boolean)
        .join("\n");
      sections.push(`## ${title} (exit ${result.code})\n\n\`\`\`text\n${output.trimEnd()}\n\`\`\`\n`);
    }
    const text = sections.join("\n").replaceAll(alice.roomId, "<alice-room>").replaceAll(bob.roomId, "<bob-room>");
    await expect(`# crew output\n\n${text}`).toMatchFileSnapshot("./__snapshots__/shim-output.md");
  });
});
