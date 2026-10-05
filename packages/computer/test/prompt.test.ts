import { AgentId, InboxRoom } from "@crew/protocol";
import { describe, expect, it } from "vitest";
import { localTimestamp, turnPrompt } from "../src/prompt";

const self = AgentId.parse("2f8c0b6e-3a1d-4c5e-9f7a-1b2c3d4e5f60");
const user = { kind: "user", id: "u", displayName: "User", handle: null } as const;
const alice = { kind: "agent", id: self, displayName: "Alice", handle: "alice" } as const;
const bob = { kind: "agent", id: "3a9d1c7f-5b2e-4f6a-8c1d-2e3f4a5b6c7d", displayName: "Bob", handle: "bob" } as const;

const direct = InboxRoom.parse({
  roomId: "6a1f4e2b-8c3d-4b5a-9e7f-0a1b2c3d4e5f",
  kind: "direct",
  name: null,
  members: [user, alice],
  parent: null,
  messages: [
    {
      id: "0b9e8d7c-6f5a-4e3d-8c2b-1a0f9e8d7c6b",
      seq: 3,
      kind: "text",
      notice: null,
      author: user,
      body: "帮我看下 loadOrders 为什么慢",
      createdAt: "2026-10-04T10:29:00.000Z",
      mentionsYou: false,
      task: null,
    },
    {
      id: "1c0f9e8d-7a6b-4f4e-9d3c-2b1a0f9e8d7c",
      seq: 4,
      kind: "text",
      notice: null,
      author: user,
      body: "顺便看看这段：\nconst a = 1;\nconst b = 2;",
      createdAt: "2026-10-04T10:29:30.000Z",
      mentionsYou: false,
      task: null,
    },
  ],
});

const group = InboxRoom.parse({
  roomId: "9c8b7a6f-5e4d-4c3b-8a2f-1e0d9c8b7a6f",
  kind: "group",
  name: "Release",
  members: [user, alice, bob],
  parent: null,
  messages: [
    {
      id: "2d1a0f9e-8b7c-4a5f-8e4d-3c2b1a0f9e8d",
      seq: 7,
      kind: "text",
      notice: null,
      author: user,
      body: "今天能发版吗？",
      createdAt: "2026-10-04T10:28:00.000Z",
      mentionsYou: false,
      task: { number: 2, status: "todo", assignee: null },
    },
    {
      id: "3e2b1a0f-9c8d-4b6a-9f5e-4d3c2b1a0f9e",
      seq: 8,
      kind: "text",
      notice: null,
      author: bob,
      body: "测试还差一项，@alice 你能看下 checkout 的用例吗？",
      createdAt: "2026-10-04T10:29:10.000Z",
      mentionsYou: true,
      task: null,
    },
  ],
});

const thread = InboxRoom.parse({
  roomId: "4f3c2b1a-0d9e-4c8b-9a7f-6e5d4c3b2a1f",
  kind: "thread",
  name: "Release",
  members: [user, alice, bob],
  parent: {
    roomId: group.roomId,
    message: {
      id: "5a4d3c2b-1e0f-4d9c-8b7a-6f5e4d3c2b1a",
      seq: 5,
      kind: "text",
      notice: null,
      author: user,
      // 太长的正文在 prompt 里截短。
      body: `回归测试的范围定一下：${"登录、注册、下单、支付、退款。".repeat(60)}`,
      createdAt: "2026-10-04T10:20:00.000Z",
      task: { number: 1, status: "in_progress", assignee: "alice" },
    },
  },
  messages: [
    {
      id: "6b5e4d3c-2f1a-4e0d-9c8b-7a6f5e4d3c2b",
      seq: 1,
      kind: "text",
      notice: null,
      author: bob,
      body: "支付我来，@alice 退款交给你？",
      createdAt: "2026-10-04T10:25:00.000Z",
      mentionsYou: true,
      task: null,
    },
    {
      id: "7c6f5e4d-3a2b-4f1e-8d9c-8b7a6f5e4d3c",
      seq: 2,
      kind: "system",
      notice: null,
      author: alice,
      body: "领取了 #1，待办 → 进行中",
      createdAt: "2026-10-04T10:26:00.000Z",
      mentionsYou: false,
      task: null,
    },
  ],
});

describe("turnPrompt", () => {
  it("matches the reviewed text", async () => {
    // 2026-10-04T18:30:00+08:00，用固定的时区偏移避免依赖运行环境的时区。
    const now = new Date("2026-10-04T10:30:00.000Z");
    const text = turnPrompt([direct, group, thread], now, self, { fresh: false, memoryBytes: 600 }).replace(
      localTimestamp(now),
      "2026-10-04T18:30:00+08:00",
    );
    await expect(text).toMatchFileSnapshot("./__snapshots__/turn-prompt.md");
  });
});

describe("turnPrompt in a new session", () => {
  const now = new Date("2026-10-04T10:30:00.000Z");

  it("asks the agent to read its memory first, and only in a new session", () => {
    expect(turnPrompt([direct], now, self, { fresh: true, memoryBytes: 600 })).toContain(
      "This is a new session: you don't remember earlier turns. Before you act, read MEMORY.md in your working directory.\n",
    );
    expect(turnPrompt([direct], now, self, { fresh: false, memoryBytes: 600 })).not.toContain("MEMORY.md");
  });

  it("asks it to trim a memory file over 16 KB", () => {
    expect(turnPrompt([direct], now, self, { fresh: true, memoryBytes: 20 * 1024 })).toContain(
      "It is 20 KB now; trim it below 16 KB",
    );
    expect(turnPrompt([direct], now, self, { fresh: true, memoryBytes: 16 * 1024 })).not.toContain("trim");
  });
});

describe("localTimestamp", () => {
  it("writes the local time with its offset, to the second", () => {
    expect(localTimestamp(new Date())).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-]\d{2}:\d{2}$/);
  });
});
