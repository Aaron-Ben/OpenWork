import { assertNever, type DesktopEvent, type RoomId } from "@crew/protocol";

/** TanStack Query 的缓存键。SSE 提示按 `keysForEvent` 让对应的键失效。 */
export const queryKeys = {
  agents: ["agents"] as const,
  groups: ["groups"] as const,
  conversations: ["conversations"] as const,
  messages: (roomId: RoomId) => ["messages", roomId] as const,
  models: ["models"] as const,
};

/**
 * 一条 SSE 失效提示对应哪些缓存需要重新获取。
 * 会话列表带着最后一条消息、未读数、名字与成员，消息、Agent 与群聊变化时都要刷新。
 * “房间有新消息”时消息本身由 `fetchNewer` 增量获取，不在这里。
 */
export function keysForEvent(event: DesktopEvent): ReadonlyArray<readonly unknown[]> {
  switch (event.type) {
    case "agents":
      return [queryKeys.agents, queryKeys.conversations];
    case "rooms":
      return [queryKeys.groups, queryKeys.conversations];
    case "room.messages":
      return [queryKeys.conversations];
    case "models":
      return [queryKeys.models];
    default:
      return assertNever(event);
  }
}
