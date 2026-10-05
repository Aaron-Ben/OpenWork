import type { Conversation, DesktopAgent } from "@crew/protocol";
import { statusIn } from "./status";

/** 侧栏会话一项的第二行：有 Agent 正在回复时写“某某 回复中…”，否则是最后一条消息的预览。 */
export function conversationPreview(
  conversation: Conversation,
  agents: readonly DesktopAgent[],
): { kind: "working" | "message" | "empty"; text: string } {
  const working = agents.filter(
    (agent) =>
      conversation.agentIds.includes(agent.id) && statusIn(agent.status, conversation.roomId).state === "working",
  );
  if (working.length > 0) {
    return { kind: "working", text: `${working.map((agent) => agent.displayName).join("、")} 回复中…` };
  }
  const last = conversation.lastMessage;
  if (!last) return { kind: "empty", text: "还没有消息" };
  // 正文压成一行；私聊里 Agent 的消息不写名字，用户的写“你”。
  const body = last.body.replace(/\s+/g, " ").trim();
  if (last.author.kind === "user") return { kind: "message", text: `你：${body}` };
  if (conversation.kind === "direct") return { kind: "message", text: body };
  return { kind: "message", text: `${last.author.displayName}：${body}` };
}

/** “消息”分段上的未读总数。 */
export function totalUnread(conversations: readonly Conversation[]): number {
  return conversations.reduce((sum, conversation) => sum + conversation.unread, 0);
}

/** 未读数的显示：超过 99 写 99+。 */
export function unreadLabel(count: number): string {
  return count > 99 ? "99+" : String(count);
}
