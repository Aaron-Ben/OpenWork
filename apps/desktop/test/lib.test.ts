import { RoomId } from "@crew/protocol";
import { describe, expect, it } from "vitest";
import { canSend, shouldSend } from "../src/lib/composer";
import { keysForEvent, queryKeys } from "../src/lib/keys";
import { selectedModel, validateNewAgent } from "../src/lib/new-agent";
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
    expect(validateNewAgent({ displayName: "Alice", persona: "代码审查者", model: "a/b" })).toEqual({});
  });

  it("reports every missing field with the server's wording", () => {
    expect(validateNewAgent({ displayName: "  ", persona: "", model: "" })).toEqual({
      displayName: "名字不能为空",
      persona: "人设不能为空",
      model: "请选择模型",
    });
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
