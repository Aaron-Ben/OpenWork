import type { DesktopEvent, RoomId } from "@crew/protocol";
import { assertNever } from "./assert";

/** TanStack Query 的缓存键。SSE 提示按 `keysForEvent` 让对应的键失效。 */
export const queryKeys = {
  agents: ["agents"] as const,
  messages: (roomId: RoomId) => ["messages", roomId] as const,
  models: ["models"] as const,
};

/** 一条 SSE 失效提示对应哪部分缓存需要重新获取。 */
export function keysForEvent(event: DesktopEvent): readonly unknown[] {
  switch (event.type) {
    case "agents":
      return queryKeys.agents;
    case "room.messages":
      return queryKeys.messages(event.roomId);
    case "models":
      return queryKeys.models;
    default:
      return assertNever(event);
  }
}
