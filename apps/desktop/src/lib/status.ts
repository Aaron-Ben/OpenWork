import { type AgentStatus, assertNever, type RoomId } from "@crew/protocol";

export type StatusTone = "working" | "idle" | "error";

/** Agent 状态在侧栏与顶栏的文案与配色。 */
export function statusView(status: AgentStatus): { label: string; tone: StatusTone } {
  switch (status.state) {
    case "idle":
      return { label: "空闲", tone: "idle" };
    case "working":
      return { label: "回复中", tone: "working" };
    case "error":
      return { label: "出错", tone: "error" };
    default:
      return assertNever(status);
  }
}

const IDLE: AgentStatus = { state: "idle" };

/**
 * Agent 在某个房间里的状态：只有这一轮涉及这个房间时才算回复中；
 * 出错时 `roomIds` 为空表示不限于某个房间（例如沙箱不可用），在每个房间都显示。
 */
export function statusIn(status: AgentStatus, roomId: RoomId): AgentStatus {
  switch (status.state) {
    case "idle":
      return status;
    case "working":
      return status.roomIds.includes(roomId) ? status : IDLE;
    case "error":
      return status.roomIds.length === 0 || status.roomIds.includes(roomId) ? status : IDLE;
    default:
      return assertNever(status);
  }
}
