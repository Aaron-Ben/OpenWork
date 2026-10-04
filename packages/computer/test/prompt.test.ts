import { InboxRoom } from "@crew/protocol";
import { describe, expect, it } from "vitest";
import { localTimestamp, turnPrompt } from "../src/prompt";

const room = InboxRoom.parse({
  roomId: "6a1f4e2b-8c3d-4b5a-9e7f-0a1b2c3d4e5f",
  kind: "direct",
  messages: [
    {
      id: "0b9e8d7c-6f5a-4e3d-8c2b-1a0f9e8d7c6b",
      seq: 3,
      author: { kind: "user", id: "u", displayName: "User" },
      body: "帮我看下 loadOrders 为什么慢",
      createdAt: "2026-10-04T10:29:00.000Z",
    },
    {
      id: "1c0f9e8d-7a6b-4f4e-9d3c-2b1a0f9e8d7c",
      seq: 4,
      author: { kind: "user", id: "u", displayName: "User" },
      body: "顺便看看这段：\nconst a = 1;\nconst b = 2;",
      createdAt: "2026-10-04T10:29:30.000Z",
    },
  ],
});

describe("turnPrompt", () => {
  it("matches the reviewed text", async () => {
    // 2026-10-04T18:30:00+08:00，用固定的时区偏移避免依赖运行环境的时区。
    const now = new Date("2026-10-04T10:30:00.000Z");
    const text = turnPrompt([room], now).replace(localTimestamp(now), "2026-10-04T18:30:00+08:00");
    await expect(text).toMatchFileSnapshot("./__snapshots__/turn-prompt.md");
  });
});

describe("localTimestamp", () => {
  it("writes the local time with its offset, to the second", () => {
    expect(localTimestamp(new Date())).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-]\d{2}:\d{2}$/);
  });
});
