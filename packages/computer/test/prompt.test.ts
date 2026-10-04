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
  messages: [
    {
      id: "0b9e8d7c-6f5a-4e3d-8c2b-1a0f9e8d7c6b",
      seq: 3,
      author: user,
      body: "帮我看下 loadOrders 为什么慢",
      createdAt: "2026-10-04T10:29:00.000Z",
      mentionsYou: false,
    },
    {
      id: "1c0f9e8d-7a6b-4f4e-9d3c-2b1a0f9e8d7c",
      seq: 4,
      author: user,
      body: "顺便看看这段：\nconst a = 1;\nconst b = 2;",
      createdAt: "2026-10-04T10:29:30.000Z",
      mentionsYou: false,
    },
  ],
});

const group = InboxRoom.parse({
  roomId: "9c8b7a6f-5e4d-4c3b-8a2f-1e0d9c8b7a6f",
  kind: "group",
  name: "Release",
  members: [user, alice, bob],
  messages: [
    {
      id: "2d1a0f9e-8b7c-4a5f-8e4d-3c2b1a0f9e8d",
      seq: 7,
      author: user,
      body: "今天能发版吗？",
      createdAt: "2026-10-04T10:28:00.000Z",
      mentionsYou: false,
    },
    {
      id: "3e2b1a0f-9c8d-4b6a-9f5e-4d3c2b1a0f9e",
      seq: 8,
      author: bob,
      body: "测试还差一项，@alice 你能看下 checkout 的用例吗？",
      createdAt: "2026-10-04T10:29:10.000Z",
      mentionsYou: true,
    },
  ],
});

describe("turnPrompt", () => {
  it("matches the reviewed text", async () => {
    // 2026-10-04T18:30:00+08:00，用固定的时区偏移避免依赖运行环境的时区。
    const now = new Date("2026-10-04T10:30:00.000Z");
    const text = turnPrompt([direct, group], now, self).replace(localTimestamp(now), "2026-10-04T18:30:00+08:00");
    await expect(text).toMatchFileSnapshot("./__snapshots__/turn-prompt.md");
  });
});

describe("localTimestamp", () => {
  it("writes the local time with its offset, to the second", () => {
    expect(localTimestamp(new Date())).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-]\d{2}:\d{2}$/);
  });
});
