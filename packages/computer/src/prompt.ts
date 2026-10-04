import type { InboxRoom } from "@crew/protocol";

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

/**
 * 每个 Turn 写给 Engine 的 prompt：唤醒说明、当前时间与按房间分组的未读消息。
 * 身份与规则在 `AGENTS.md` 里，这里不重复。多行正文的后续行缩进四格，保持在消息之下。
 */
export function turnPrompt(rooms: InboxRoom[], now: Date): string {
  const sections = rooms.map((room) => {
    const lines = room.messages.map((message) => {
      const [first = "", ...rest] = message.body.split("\n");
      const head = `  [${message.id}] ${message.author.displayName} (${message.author.kind}): ${first}`;
      return [head, ...rest.map((line) => `    ${line}`)].join("\n");
    });
    return [`# ${room.roomId} [${room.kind}]`, ...lines].join("\n");
  });
  return `You've been woken because there are new messages in your Crew rooms. Reply with \`crew reply\` if you have something useful to say.

Current time: ${localTimestamp(now)}

Your unread messages:
${sections.join("\n\n")}
`;
}
