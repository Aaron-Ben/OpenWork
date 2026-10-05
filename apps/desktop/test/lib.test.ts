import {
  AgentId,
  type AgentStatus,
  type Conversation,
  type DesktopAgent,
  type DesktopGroup,
  MessageId,
  RoomId,
  type RoomMessage,
  type TaskStatus,
  type TaskView,
} from "@crew/protocol";
import { describe, expect, it } from "vitest";
import { AVATAR_COLORS, avatarColor, avatarInitial, RING_MAX, ringSlots } from "../src/lib/avatar";
import { canSend, shouldSend, shouldSubmit } from "../src/lib/composer";
import { conversationPreview, totalUnread, unreadLabel } from "../src/lib/conversations";
import { keysForEvent, queryKeys } from "../src/lib/keys";
import { rehypeMentions, splitMentions } from "../src/lib/mentions";
import { hasOlder, mergeMessages, newestSeq } from "../src/lib/messages";
import { selectedModel, suggestHandle, validateNewAgent } from "../src/lib/new-agent";
import { groupMembers, groupsWithout, nonMembers, toggle, validateNewGroup } from "../src/lib/new-group";
import { isLate, noticeLook, noticeParts } from "../src/lib/notices";
import { isNearBottom } from "../src/lib/scroll";
import { statusIn, statusView } from "../src/lib/status";
import { groupByStatus, nextStatuses, openCount } from "../src/lib/tasks";
import { formatListTime, formatMessageTime } from "../src/lib/time";

// 界面里抽出来的纯逻辑。组件本身不写只断言 HTML 的测试。

const roomId = RoomId.parse("6a1f4e2b-8c3d-4b5a-9e7f-0a1b2c3d4e5f");

describe("keysForEvent", () => {
  it("refreshes the agent and conversation lists when agents change", () => {
    expect(keysForEvent({ type: "agents" })).toEqual([queryKeys.agents, queryKeys.conversations, queryKeys.memories]);
  });

  it("refreshes the conversation list, the room's threads and its tasks when it has new messages; the messages come from fetchNewer", () => {
    expect(keysForEvent({ type: "room.messages", roomId })).toEqual([
      queryKeys.conversations,
      queryKeys.threadList(roomId),
      queryKeys.tasks(roomId),
    ]);
  });

  it("refreshes the group and conversation lists when groups or their members change", () => {
    expect(keysForEvent({ type: "rooms" })).toEqual([queryKeys.groups, queryKeys.conversations]);
  });

  it("refreshes every run list and run detail when a run changes", () => {
    expect(keysForEvent({ type: "run.activity", runId: "r", roomIds: [roomId] })).toEqual([queryKeys.runs]);
  });

  it("refreshes the model list when the computer reports models", () => {
    expect(keysForEvent({ type: "models" })).toEqual([queryKeys.models]);
  });
});

describe("statusView", () => {
  it("labels each state", () => {
    expect(statusView({ state: "idle" })).toEqual({ label: "空闲", tone: "idle" });
    expect(statusView({ state: "working", runId: "r", roomIds: [roomId] })).toEqual({
      label: "回复中",
      tone: "working",
    });
    expect(statusView({ state: "error", reason: "OpenCode 未登录", roomIds: [] })).toEqual({
      label: "出错",
      tone: "error",
    });
  });
});

describe("statusIn", () => {
  const other = RoomId.parse("7a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d");

  it("shows working only in the rooms the run is about", () => {
    const working: AgentStatus = { state: "working", runId: "r", roomIds: [roomId] };
    expect(statusIn(working, roomId)).toEqual(working);
    expect(statusIn(working, other)).toEqual({ state: "idle" });
  });

  it("shows an error in its rooms, or everywhere when it is not tied to a room", () => {
    const failed: AgentStatus = { state: "error", reason: "限流", roomIds: [roomId] };
    expect(statusIn(failed, other)).toEqual({ state: "idle" });
    const blocked: AgentStatus = { state: "error", reason: "沙箱不可用", roomIds: [] };
    expect(statusIn(blocked, other)).toEqual(blocked);
  });
});

describe("shouldSubmit", () => {
  it("submits a dialog on Enter, but not again while the first request is still pending", () => {
    const enter = { key: "Enter", shiftKey: false, isComposing: false, keyCode: 13 };
    expect(shouldSubmit(enter, false)).toBe(true);
    expect(shouldSubmit(enter, true)).toBe(false);
    expect(shouldSubmit({ ...enter, isComposing: true }, false)).toBe(false);
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

  it("offers only the groups an agent is not in yet", () => {
    const other = { id: roomId, agentIds: [agentId(2)] } as DesktopGroup;
    expect(groupsWithout(agentId(1), [group, other])).toEqual([other]);
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

describe("avatars", () => {
  it("give an agent the same color everywhere, from the palette", () => {
    expect(avatarColor("alice")).toBe(avatarColor("alice"));
    expect(AVATAR_COLORS).toContain(avatarColor("bob"));
  });

  it("show the first character, uppercase for latin letters", () => {
    expect(avatarInitial("alice")).toBe("A");
    expect(avatarInitial(" 小明")).toBe("小");
    expect(avatarInitial("")).toBe("?");
  });

  it("place at most five members on a ring inside the avatar", () => {
    expect(ringSlots(0)).toEqual([]);
    expect(ringSlots(1)).toEqual([{ x: 0.5, y: 0.5, size: 0.62 }]);
    const slots = ringSlots(9);
    expect(slots).toHaveLength(RING_MAX);
    for (const slot of slots) {
      expect(slot.x - slot.size / 2).toBeGreaterThanOrEqual(-1e-9);
      expect(slot.x + slot.size / 2).toBeLessThanOrEqual(1 + 1e-9);
      expect(slot.y - slot.size / 2).toBeGreaterThanOrEqual(-1e-9);
      expect(slot.y + slot.size / 2).toBeLessThanOrEqual(1 + 1e-9);
    }
    // 相邻的头像不重叠。
    for (const [i, slot] of slots.entries()) {
      const next = slots[(i + 1) % slots.length];
      if (!next) continue;
      expect(Math.hypot(slot.x - next.x, slot.y - next.y)).toBeGreaterThanOrEqual(slot.size);
    }
    // 两个成员沿对角线放。
    const [first, second] = ringSlots(2);
    expect(first && second && first.x < second.x && first.y < second.y).toBe(true);
    // 三个及以上从正上方开始。
    expect(slots[0]?.x).toBeCloseTo(0.5);
    expect(slots[0]?.y).toBeLessThan(0.5);
  });
});

describe("conversationPreview", () => {
  const alice = { id: agentId(1), displayName: "Alice", status: { state: "idle" } } as DesktopAgent;
  const bob = {
    id: agentId(2),
    displayName: "Bob",
    status: { state: "working", runId: "r", roomIds: [roomId] },
  } as DesktopAgent;
  const base = { roomId, name: "发版", unread: 0, activeAt: "", agentIds: [agentId(1)] };
  const last = (kind: "user" | "agent", displayName: string, body: string) => ({
    author: { kind, id: "x", displayName, handle: null },
    body,
    createdAt: "",
  });

  it("says who is replying instead of the last message", () => {
    const group = { ...base, kind: "group", agentIds: [agentId(1), agentId(2)], lastMessage: null } as Conversation;
    expect(conversationPreview(group, [alice, bob])).toEqual({ kind: "working", text: "Bob 回复中…" });
  });

  it("names the author in a group, writes 你 for the user, and keeps it on one line", () => {
    const group = { ...base, kind: "group", lastMessage: last("agent", "Alice", "第一行\n第二行") } as Conversation;
    expect(conversationPreview(group, [alice]).text).toBe("Alice：第一行 第二行");
    const mine = { ...base, kind: "direct", lastMessage: last("user", "User", "在吗") } as Conversation;
    expect(conversationPreview(mine, [alice]).text).toBe("你：在吗");
    const direct = { ...base, kind: "direct", lastMessage: last("agent", "Alice", "在") } as Conversation;
    expect(conversationPreview(direct, [alice]).text).toBe("在");
  });

  it("says there is nothing yet for an empty room", () => {
    const empty = { ...base, kind: "direct", lastMessage: null } as Conversation;
    expect(conversationPreview(empty, [alice])).toEqual({ kind: "empty", text: "还没有消息" });
  });

  it("adds up unread counts and caps the label at 99+", () => {
    expect(totalUnread([{ unread: 2 }, { unread: 3 }] as Conversation[])).toBe(5);
    expect(unreadLabel(7)).toBe("7");
    expect(unreadLabel(120)).toBe("99+");
  });
});

describe("mentions", () => {
  const handles = new Set(["alice", "bob"]);

  it("splits out known handles and leaves the rest as text", () => {
    expect(splitMentions("@Alice 看一下，@carol 不认识，me@bob.com 不算", handles)).toEqual([
      { mention: "alice", text: "@Alice" },
      { text: " 看一下，@carol 不认识，me@bob.com 不算" },
    ]);
  });

  it("keeps a trailing hyphen out of the mention", () => {
    expect(splitMentions("找@bob-谢谢", handles)).toEqual([
      { text: "找" },
      { mention: "bob", text: "@bob" },
      { text: "-谢谢" },
    ]);
  });

  it("wraps mentions in the rendered tree, but not inside code or links", () => {
    const tree = {
      type: "root",
      children: [
        { type: "element", tagName: "p", children: [{ type: "text", value: "@alice 你好" }] },
        { type: "element", tagName: "code", children: [{ type: "text", value: "@alice" }] },
        { type: "element", tagName: "a", children: [{ type: "text", value: "@bob" }] },
      ],
    };
    rehypeMentions({ handles })(tree);
    expect(tree.children[0]).toEqual({
      type: "element",
      tagName: "p",
      children: [
        {
          type: "element",
          tagName: "span",
          properties: { className: ["mention"] },
          children: [{ type: "text", value: "@alice" }],
        },
        { type: "text", value: " 你好" },
      ],
    });
    expect(tree.children[1]?.children).toEqual([{ type: "text", value: "@alice" }]);
    expect(tree.children[2]?.children).toEqual([{ type: "text", value: "@bob" }]);
  });
});

describe("formatListTime", () => {
  const now = new Date(2026, 9, 5, 20, 0);

  it("writes the clock today, 昨天 yesterday, the date this year, and the full date before", () => {
    expect(formatListTime(new Date(2026, 9, 5, 9, 5).toISOString(), now)).toBe("09:05");
    expect(formatListTime(new Date(2026, 9, 4, 23, 0).toISOString(), now)).toBe("昨天");
    expect(formatListTime(new Date(2026, 8, 30, 8, 0).toISOString(), now)).toBe("9月30日");
    expect(formatListTime(new Date(2025, 11, 31, 8, 0).toISOString(), now)).toBe("2025/12/31");
  });
});

describe("task helpers", () => {
  const alice = { id: AgentId.parse("2f8c0b6e-3a1d-4c5e-9f7a-1b2c3d4e5f60"), displayName: "Alice", handle: "alice" };
  const task = (number: number, status: TaskStatus, assigned: boolean): TaskView => ({
    id: `00000000-0000-4000-8000-00000000000${number}`,
    roomId,
    number,
    title: `任务 ${number}`,
    status,
    assignee: assigned ? alice : null,
    messageId: MessageId.parse(`10000000-0000-4000-8000-00000000000${number}`),
    threadId: null,
    createdAt: "2026-10-05T10:00:00.000Z",
    updatedAt: "2026-10-05T10:00:00.000Z",
  });

  it("groups by status in number order and counts unfinished tasks", () => {
    const tasks = [task(3, "todo", false), task(1, "todo", false), task(2, "done", true)];
    expect(
      groupByStatus(tasks)
        .get("todo")
        ?.map((t) => t.number),
    ).toEqual([1, 3]);
    expect(openCount(tasks)).toBe(2);
  });

  it("offers only the statuses the transition table allows, and none that need an assignee when there is none", () => {
    expect(nextStatuses({ status: "todo", assignee: null })).toEqual(["closed"]);
    expect(nextStatuses({ status: "todo", assignee: alice })).toEqual(["in_progress", "closed"]);
    expect(nextStatuses({ status: "done", assignee: null })).toEqual(["todo", "closed"]);
  });
});

describe("noticeLook", () => {
  it("colors task changes blue, completion green, send-backs amber and reminders violet", () => {
    expect(noticeLook({ type: "task.created", number: 1, assignee: null })).toEqual({
      icon: "clipboard",
      tone: "task",
    });
    expect(noticeLook({ type: "task.claimed", number: 1 })).toEqual({ icon: "play", tone: "task" });
    const status = { type: "task.status", number: 1, from: "in_progress", sentBack: false } as const;
    expect(noticeLook({ ...status, to: "in_review" })).toEqual({ icon: "eye", tone: "task" });
    expect(noticeLook({ ...status, to: "done" })).toEqual({ icon: "check", tone: "ok" });
    expect(noticeLook({ ...status, to: "closed" })).toEqual({ icon: "closed", tone: "muted" });
    expect(noticeLook({ ...status, from: "in_review", to: "in_progress", sentBack: true })).toEqual({
      icon: "sendBack",
      tone: "warn",
    });
    const reminder = {
      type: "reminder",
      title: "看 CI",
      repeat: null,
      setAt: "",
      dueAt: "2026-10-05T10:00:00.000Z",
    } as const;
    expect(noticeLook(reminder)).toEqual({ icon: "alarm", tone: "violet" });
    expect(noticeLook(null)).toEqual({ icon: "dot", tone: "muted" });
  });

  it("calls a reminder late only when it fired more than a minute after it was due", () => {
    const reminder = {
      type: "reminder",
      title: "看 CI",
      repeat: null,
      setAt: "",
      dueAt: "2026-10-05T10:00:00.000Z",
    } as const;
    expect(isLate(reminder, "2026-10-05T10:00:50.000Z")).toBe(false);
    expect(isLate(reminder, "2026-10-05T11:00:00.000Z")).toBe(true);
  });

  it("takes a status change's note out of the notice line, so it shows once, in the quote below", () => {
    const sentBack = {
      type: "task.status",
      number: 3,
      from: "in_review",
      to: "in_progress",
      sentBack: true,
      note: "标题太长",
    } as const;
    expect(noticeParts("把 #3 从待审改成进行中，@alice：标题太长", sentBack)).toEqual({
      line: "把 #3 从待审改成进行中，@alice",
      note: "标题太长",
    });
    // 没有说明，或正文不是以说明结尾时，整行照旧。
    expect(noticeParts("把 #3 从待审改成进行中", { ...sentBack, note: undefined })).toEqual({
      line: "把 #3 从待审改成进行中",
    });
    expect(noticeParts("别的文字", sentBack)).toEqual({ line: "别的文字" });
    expect(noticeParts("领取了 #3", null)).toEqual({ line: "领取了 #3" });
  });
});
