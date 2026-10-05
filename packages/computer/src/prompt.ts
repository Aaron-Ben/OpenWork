import {
  type AgentId,
  assertNever,
  clipText,
  type InboxRoom,
  type MessageView,
  type Participant,
  type TaskTag,
} from "@crew/protocol";

/** 带本地时区偏移的 RFC 3339 时间，精确到秒，例如 `2026-10-04T18:30:00+08:00`。 */
export function localTimestamp(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  const offset = -date.getTimezoneOffset();
  const sign = offset >= 0 ? "+" : "-";
  const abs = Math.abs(offset);
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
    `T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}` +
    `${sign}${pad(Math.floor(abs / 60))}:${pad(abs % 60)}`
  );
}

/** 参与者在 prompt 里的写法：用户是 `User (user)`，Agent 是 `Bob (@bob)`，本 Agent 加上 `you`。 */
export function participantLabel(participant: Participant, self?: AgentId): string {
  if (participant.kind === "user") return `${participant.displayName} (user)`;
  const you = participant.id === self ? ", you" : "";
  return `${participant.displayName} (@${participant.handle ?? "?"}${you})`;
}

/** 任务后缀：`[task #3 in_progress, assigned to @alice]`。 */
function taskSuffix(task: TaskTag): string {
  return ` [task #${task.number} ${task.status}, ${task.assignee ? `assigned to @${task.assignee}` : "unassigned"}]`;
}

/**
 * 一条消息：`[id] 作者: 正文`。多行正文的后续行缩进四格，保持在消息之下。
 * 通知标 `[notice]`，作者是做这件事的人；任务的宿主消息在正文后面带任务后缀。
 */
export function messageLines(
  message: MessageView & { mentionsYou?: boolean; task?: TaskTag | null },
  self?: AgentId,
): string {
  const [first = "", ...rest] = message.body.split("\n");
  const notice = message.kind === "system" ? " [notice]" : "";
  const mention = message.mentionsYou ? " [mentions you]" : "";
  const task = message.task ? taskSuffix(message.task) : "";
  const tail = rest.length === 0 ? task : "";
  const head = `  [${message.id}] ${participantLabel(message.author, self)}${notice}${mention}: ${first}${tail}`;
  const body = rest.map((line, index) => `    ${line}${index === rest.length - 1 ? task : ""}`);
  return [head, ...body].join("\n");
}

/** 讨论串挂着的那条消息在 prompt 里最多这么多字符：完整的内容在群聊里，这里只提示讨论串在说什么。 */
export const THREAD_PARENT_MAX = 600;

function roomHeading(room: InboxRoom): string {
  switch (room.kind) {
    case "group":
      return `# Room ${room.roomId} (group "${room.name ?? ""}")`;
    case "direct":
      return `# Room ${room.roomId} (direct)`;
    case "thread":
      return `# Thread ${room.roomId} (in group "${room.name ?? ""}")`;
    default:
      return assertNever(room.kind);
  }
}

/** 讨论串的开头：它挂在群聊里的哪条消息下。正文太长时截短。 */
function threadParent(room: InboxRoom, self: AgentId): string[] {
  if (!room.parent) return [];
  const { message } = room.parent;
  const body = clipText(message.body, THREAD_PARENT_MAX);
  const clipped = body.length < message.body.length ? { ...message, body: `${body}…` } : message;
  return [`Under this message in room ${room.parent.roomId}:`, messageLines(clipped, self), "Unread replies:"];
}

/**
 * 每个 Turn 写给 Engine 的 prompt：唤醒说明、当前时间与按房间分组的未读消息。
 * 群聊与讨论串附上成员名册，讨论串还附上它挂着的那条消息。身份与规则在 `AGENTS.md` 里，这里不重复。
 */
export function turnPrompt(rooms: InboxRoom[], now: Date, self: AgentId): string {
  const sections = rooms.map((room) => {
    const roster =
      room.kind === "direct"
        ? []
        : [`Members: ${room.members.map((member) => participantLabel(member, self)).join(", ")}`];
    return [
      roomHeading(room),
      ...roster,
      ...threadParent(room, self),
      ...room.messages.map((message) => messageLines(message, self)),
    ].join("\n");
  });
  return `You've been woken because there are new messages in your Crew rooms. Reply with \`crew reply\` if you have something useful to say.

Current time: ${localTimestamp(now)}

Your unread messages:

${sections.join("\n\n")}
`;
}
