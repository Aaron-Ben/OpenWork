import { AgentId, type DesktopAgent, type DesktopGroup, RoomId, type RoomMessage } from "@crew/protocol";
import { describe, expect, it } from "vitest";
import { canSend, shouldSend } from "../src/lib/composer";
import { keysForEvent, queryKeys } from "../src/lib/keys";
import { hasOlder, mergeMessages, newestSeq } from "../src/lib/messages";
import { selectedModel, suggestHandle, validateNewAgent } from "../src/lib/new-agent";
import { groupMembers, nonMembers, toggle, validateNewGroup } from "../src/lib/new-group";
import { isNearBottom } from "../src/lib/scroll";
import { statusView } from "../src/lib/status";
import { formatMessageTime } from "../src/lib/time";

// 界面里抽出来的纯逻辑。组件本身不写只断言 HTML 的测试。

const roomId = RoomId.parse("6a1f4e2b-8c3d-4b5a-9e7f-0a1b2c3d4e5f");

describe("keysForEvent", () => {
  it("refreshes the agent list when agents change", () => {
    expect(keysForEvent({ type: "agents" })).toEqual(queryKeys.agents);
  });

  it("refreshes only the room that has new messages", () => {
    expect(keysForEvent({ type: "room.messages", roomId })).toEqual(["messages", roomId]);
  });

  it("refreshes the group list when groups or their members change", () => {
    expect(keysForEvent({ type: "rooms" })).toEqual(queryKeys.groups);
  });

  it("refreshes the model list when the computer reports models", () => {
    expect(keysForEvent({ type: "models" })).toEqual(queryKeys.models);
  });
});

describe("statusView", () => {
  it("labels each state", () => {
    expect(statusView({ state: "idle" })).toEqual({ label: "空闲", tone: "idle" });
    expect(statusView({ state: "working" })).toEqual({ label: "回复中", tone: "working" });
    expect(statusView({ state: "error", reason: "OpenCode 未登录" })).toEqual({ label: "出错", tone: "error" });
  });
});

describe("shouldSend", () => {
  const key = { key: "Enter", shiftKey: false, isComposing: false, keyCode: 13 };

  it("sends on Enter", () => {
    expect(shouldSend(key)).toBe(true);
  });

  it("inserts a newline on Shift+Enter", () => {
    expect(shouldSend({ ...key, shiftKey: true })).toBe(false);
  });

  it("leaves Enter to the input method while it is composing", () => {
    expect(shouldSend({ ...key, isComposing: true })).toBe(false);
    expect(shouldSend({ ...key, keyCode: 229 })).toBe(false);
  });

  it("ignores other keys", () => {
    expect(shouldSend({ ...key, key: "a", keyCode: 65 })).toBe(false);
  });
});

describe("canSend", () => {
  it("refuses a draft that is only whitespace", () => {
    expect(canSend(" \n\t")).toBe(false);
    expect(canSend(" hi ")).toBe(true);
  });
});

describe("isNearBottom", () => {
  it("treats the last 80 pixels as the bottom", () => {
    expect(isNearBottom({ scrollTop: 920, scrollHeight: 1500, clientHeight: 500 })).toBe(true);
    expect(isNearBottom({ scrollTop: 919, scrollHeight: 1500, clientHeight: 500 })).toBe(false);
  });

  it("treats content shorter than the viewport as the bottom", () => {
    expect(isNearBottom({ scrollTop: 0, scrollHeight: 300, clientHeight: 500 })).toBe(true);
  });
});

describe("formatMessageTime", () => {
  // 用本地时间构造，结果不依赖运行环境的时区。
  const now = new Date(2026, 9, 4, 18, 30);

  it("shows only the time for today", () => {
    expect(formatMessageTime(new Date(2026, 9, 4, 9, 5).toISOString(), now)).toBe("09:05");
  });

  it("adds the month and day for an earlier day this year", () => {
    expect(formatMessageTime(new Date(2026, 9, 3, 23, 59).toISOString(), now)).toBe("10月3日 23:59");
  });

  it("adds the year for an earlier year", () => {
    expect(formatMessageTime(new Date(2025, 11, 31, 8, 0).toISOString(), now)).toBe("2025年12月31日 08:00");
  });
});

describe("validateNewAgent", () => {
  it("accepts a complete form", () => {
    expect(validateNewAgent({ displayName: "Alice", handle: "alice", persona: "代码审查者", model: "a/b" })).toEqual(
      {},
    );
  });

  it("reports every missing field with the server's wording", () => {
    expect(validateNewAgent({ displayName: "  ", handle: "", persona: "", model: "" })).toEqual({
      displayName: "名字不能为空",
      handle: "handle 不能为空",
      persona: "人设不能为空",
      model: "请选择模型",
    });
  });

  it("reports a handle in the wrong format with the server's wording", () => {
    expect(validateNewAgent({ displayName: "A", handle: "Alice", persona: "p", model: "a/b" })).toEqual({
      handle: "handle 只能用小写字母、数字与 -，并以字母或数字开头",
    });
  });
});

describe("suggestHandle", () => {
  it("lowercases the name and joins words with hyphens", () => {
    expect(suggestHandle("Code Reviewer!")).toBe("code-reviewer");
    expect(suggestHandle("  Alice ")).toBe("alice");
  });

  it("gives nothing for a name without latin letters or digits", () => {
    expect(suggestHandle("小明")).toBe("");
  });

  it("keeps the latin part of a mixed name and stays within the length limit", () => {
    expect(suggestHandle("测试 QA")).toBe("qa");
    expect(suggestHandle("a".repeat(40))).toHaveLength(32);
  });
});

const agentId = (n: number) => AgentId.parse(`00000000-0000-4000-8000-00000000000${n}`);
const agent = (n: number) => ({ id: agentId(n) }) as DesktopAgent;
const group = { agentIds: [agentId(1), agentId(3)] } as DesktopGroup;

describe("group members", () => {
  it("lists members and non-members in the agent list's order", () => {
    const agents = [agent(1), agent(2), agent(3)];
    expect(groupMembers(group, agents).map((a) => a.id)).toEqual([agentId(1), agentId(3)]);
    expect(nonMembers(group, agents).map((a) => a.id)).toEqual([agentId(2)]);
  });

  it("toggles a choice on and off", () => {
    expect(toggle([agentId(1)], agentId(2))).toEqual([agentId(1), agentId(2)]);
    expect(toggle([agentId(1), agentId(2)], agentId(1))).toEqual([agentId(2)]);
  });

  it("requires a name and at least one agent", () => {
    expect(validateNewGroup({ name: " ", agentIds: [] })).toEqual({
      name: "群聊名字不能为空",
      agentIds: "至少选择一个 agent",
    });
    expect(validateNewGroup({ name: "发版", agentIds: [agentId(1)] })).toEqual({});
  });
});

const message = (seq: number, body = `第 ${seq} 条`) => ({ seq, body }) as RoomMessage;

describe("mergeMessages", () => {
  it("appends newer messages and prepends older ones in sequence order", () => {
    const current = [message(5), message(6)];
    expect(mergeMessages(current, [message(7)]).map((m) => m.seq)).toEqual([5, 6, 7]);
    expect(mergeMessages(current, [message(3), message(4)]).map((m) => m.seq)).toEqual([3, 4, 5, 6]);
  });

  it("drops duplicates when batches overlap, keeping the newer copy", () => {
    const merged = mergeMessages([message(1), message(2)], [message(2, "新"), message(3)]);
    expect(merged.map((m) => [m.seq, m.body])).toEqual([
      [1, "第 1 条"],
      [2, "新"],
      [3, "第 3 条"],
    ]);
  });

  it("starts from nothing when there is no cache yet", () => {
    expect(mergeMessages(undefined, [message(2), message(1)]).map((m) => m.seq)).toEqual([1, 2]);
  });
});

describe("hasOlder and newestSeq", () => {
  it("knows there is more before the first message unless it is message 1", () => {
    expect(hasOlder([message(2), message(3)])).toBe(true);
    expect(hasOlder([message(1)])).toBe(false);
    expect(hasOlder([])).toBe(false);
  });

  it("gives the newest sequence number, or 0 for an empty room", () => {
    expect(newestSeq([message(1), message(4)])).toBe(4);
    expect(newestSeq([])).toBe(0);
  });
});

describe("selectedModel", () => {
  const models = ["a/one", "b/two"];

  it("keeps the model the user chose while it is still listed", () => {
    expect(selectedModel("b/two", models)).toBe("b/two");
  });

  it("falls back to the first model when nothing or an empty value was chosen", () => {
    expect(selectedModel(undefined, models)).toBe("a/one");
    expect(selectedModel("", models)).toBe("a/one");
  });

  it("falls back to the first model when the chosen one is no longer listed", () => {
    expect(selectedModel("c/gone", models)).toBe("a/one");
  });

  it("is empty while there are no models", () => {
    expect(selectedModel(undefined, [])).toBe("");
  });
});
