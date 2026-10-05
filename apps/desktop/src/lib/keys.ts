import { assertNever, type DesktopEvent, type RoomId } from "@crew/protocol";

/** TanStack Query 的缓存键。SSE 提示按 `keysForEvent` 让对应的键失效。 */
export const queryKeys = {
  agents: ["agents"] as const,
  groups: ["groups"] as const,
  conversations: ["conversations"] as const,
  messages: (roomId: RoomId) => ["messages", roomId] as const,
  /** 全部讨论串列表的前缀：用户读了一个讨论串后，所在群聊的未读数随之变化。 */
  threads: ["threads"] as const,
  threadList: (roomId: RoomId) => ["threads", roomId] as const,
  tasks: (roomId: RoomId) => ["tasks", roomId] as const,
  models: ["models"] as const,
  /** 全部运行记录列表的前缀：让它失效时，按房间或按 Agent 的列表一起失效。 */
  runs: ["runs"] as const,
  runList: (filter: { roomId?: RoomId; agentId?: string }) => ["runs", "list", filter] as const,
  run: (runId: string) => ["runs", "detail", runId] as const,
};

/**
 * 一条 SSE 失效提示对应哪些缓存需要重新获取。
 * 会话列表带着最后一条消息、未读数、名字与成员，消息、Agent 与群聊变化时都要刷新。
 * “房间有新消息”时消息本身由 `fetchNewer` 增量获取，不在这里；讨论串的消息让它所在群聊的讨论串摘要也变了，
 * Server 同时为群聊发一条提示。
 */
export function keysForEvent(event: DesktopEvent): ReadonlyArray<readonly unknown[]> {
  switch (event.type) {
    case "agents":
      return [queryKeys.agents, queryKeys.conversations];
    case "rooms":
      return [queryKeys.groups, queryKeys.conversations];
    case "room.messages":
      // 任务的每次改动都写一条通知：任务列表跟着房间的新消息刷新。
      return [queryKeys.conversations, queryKeys.threadList(event.roomId), queryKeys.tasks(event.roomId)];
    case "models":
      return [queryKeys.models];
    case "run.activity":
      return [queryKeys.runs];
    default:
      return assertNever(event);
  }
}
