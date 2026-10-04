import type { AgentId, InboxRoom, MessageView, Participant } from "@crew/protocol";

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

/** 一条消息：`[id] 作者: 正文`。多行正文的后续行缩进四格，保持在消息之下。 */
export function messageLines(message: MessageView & { mentionsYou?: boolean }, self?: AgentId): string {
  const [first = "", ...rest] = message.body.split("\n");
  const mention = message.mentionsYou ? " [mentions you]" : "";
  const head = `  [${message.id}] ${participantLabel(message.author, self)}${mention}: ${first}`;
  return [head, ...rest.map((line) => `    ${line}`)].join("\n");
}

function roomHeading(room: InboxRoom): string {
  return room.kind === "group"
    ? `# Room ${room.roomId} (group "${room.name ?? ""}")`
    : `# Room ${room.roomId} (direct)`;
}

/**
 * 每个 Turn 写给 Engine 的 prompt：唤醒说明、当前时间与按房间分组的未读消息。
 * 群聊附上成员名册。身份与规则在 `AGENTS.md` 里，这里不重复。
 */
export function turnPrompt(rooms: InboxRoom[], now: Date, self: AgentId): string {
  const sections = rooms.map((room) => {
    const roster =
      room.kind === "group"
        ? [`Members: ${room.members.map((member) => participantLabel(member, self)).join(", ")}`]
        : [];
    return [roomHeading(room), ...roster, ...room.messages.map((message) => messageLines(message, self))].join("\n");
  });
  return `You've been woken because there are new messages in your Crew rooms. Reply with \`crew reply\` if you have something useful to say.

Current time: ${localTimestamp(now)}

Your unread messages:

${sections.join("\n\n")}
`;
}
