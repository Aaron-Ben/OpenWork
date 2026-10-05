import type { RunEvent } from "@crew/protocol";
import { MessageId, RoomId } from "@crew/protocol";
import { describe, expect, it } from "vitest";
import {
  cacheHitRate,
  describeTool,
  formatCost,
  formatDuration,
  formatTokens,
  isReplyCommand,
  liveView,
  runOutcome,
  timeline,
  triggerText,
} from "../src/lib/runs";

// 运行记录在界面上的写法。

const roomId = RoomId.parse("6a1f4e2b-8c3d-4b5a-9e7f-0a1b2c3d4e5f");
const at = "2026-10-05T12:00:00.000Z";
const tool = (seq: number, tool: string, input: object, title = "") =>
  ({
    seq,
    kind: "tool",
    at,
    tool,
    title,
    input: JSON.stringify(input),
    output: "",
    durationMs: 15,
    failed: false,
  }) as const;

describe("runOutcome", () => {
  it("calls a successful run without replies 白跑", () => {
    expect(runOutcome({ outcome: "succeeded", replies: 2 })).toEqual({ label: "发出 2 条", tone: "ok" });
    expect(runOutcome({ outcome: "succeeded", replies: 0 })).toEqual({ label: "白跑", tone: "idle" });
  });

  it("labels the other outcomes", () => {
    expect(runOutcome({ outcome: "running", replies: 0 }).label).toBe("进行中");
    expect(runOutcome({ outcome: "failed", replies: 0 }).tone).toBe("error");
    expect(runOutcome({ outcome: "interrupted", replies: 1 }).label).toBe("中断");
  });
});

describe("numbers", () => {
  it("formats durations, tokens and costs compactly", () => {
    expect(formatDuration(41_400)).toBe("41s");
    expect(formatDuration(125_000)).toBe("2m05s");
    expect(formatTokens(950)).toBe("950");
    expect(formatTokens(18_240)).toBe("18.2k");
    expect(formatCost(0.0042)).toBe("$0.0042");
    expect(formatCost(1.5)).toBe("$1.50");
    expect(formatCost(0)).toBe("—");
  });

  it("computes the share of input served from cache", () => {
    const usage = { input: 200, output: 10, reasoning: 0, cacheRead: 800, cacheWrite: 0, cost: 0 };
    expect(cacheHitRate(usage)).toBe(0.8);
    expect(cacheHitRate({ ...usage, input: 0, cacheRead: 0 })).toBeNull();
  });
});

describe("describeTool", () => {
  it("shows commands with $, reads and writes by their last two path segments", () => {
    expect(describeTool(tool(1, "bash", { command: "git log -5" }))).toEqual({ mark: "$", text: "git log -5" });
    expect(describeTool(tool(1, "read", { filePath: "/var/x/agents/a/work/notes.md" }))).toEqual({
      mark: "↗",
      text: "读取 work/notes.md",
    });
    expect(describeTool(tool(1, "write", { filePath: "/w/plan.md" })).text).toBe("写入 w/plan.md");
  });

  it("falls back to the tool name and title for other tools and clipped input", () => {
    expect(describeTool(tool(1, "webfetch", {}, "https://example.com"))).toEqual({
      mark: "·",
      text: "webfetch https://example.com",
    });
    expect(describeTool({ tool: "bash", title: "ls", input: '{"command":"l' })).toEqual({ mark: "$", text: "ls" });
  });
});

describe("timeline and liveView", () => {
  const events: RunEvent[] = [
    { seq: 1, kind: "step", at },
    tool(2, "bash", { command: "ls" }),
    tool(3, "read", { filePath: "/w/a.txt" }),
    {
      seq: 4,
      kind: "step_end",
      at,
      usage: { input: 1, output: 1, reasoning: 0, cacheRead: 0, cacheWrite: 0, cost: 0 },
    },
    { seq: 5, kind: "step", at },
    {
      seq: 6,
      kind: "reply",
      at,
      roomId,
      messageId: MessageId.parse("0b9e8d7c-6f5a-4e3d-8c2b-1a0f9e8d7c6b"),
      body: "好",
    },
    tool(7, "bash", { command: "crew reply 6a1f <<'EOF'\n好\nEOF" }),
  ];

  it("drops the crew reply command, which the reply event already shows", () => {
    expect(isReplyCommand(events[6] as RunEvent)).toBe(true);
    expect(timeline(events).map((event) => event.seq)).toEqual([1, 2, 3, 4, 5, 6]);
  });

  it("shows the latest tool calls and thinking while a step is open", () => {
    expect(liveView({ events })).toEqual({
      recent: [
        { seq: 2, mark: "$", text: "ls", duration: "15ms" },
        { seq: 3, mark: "↗", text: "读取 w/a.txt", duration: "15ms" },
      ],
      thinking: true,
      steps: 2,
    });
    expect(liveView({ events: events.slice(0, 4) }).thinking).toBe(false);
  });
});

describe("triggerText", () => {
  it("names the messages that woke a run", () => {
    const other = RoomId.parse("7a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d");
    expect(triggerText([{ roomId, fromSeq: 3, toSeq: 3, parentRoomId: null }])).toBe("第 3 条消息");
    expect(triggerText([{ roomId, fromSeq: 3, toSeq: 5, parentRoomId: null }])).toBe("第 3–5 条消息");
    expect(triggerText([{ roomId: other, fromSeq: 2, toSeq: 2, parentRoomId: roomId }])).toBe("讨论串里第 2 条消息");
    expect(
      triggerText([
        { roomId, fromSeq: 1, toSeq: 1, parentRoomId: null },
        { roomId: other, fromSeq: 2, toSeq: 2, parentRoomId: null },
      ]),
    ).toBe("2 个房间的消息");
  });
});
