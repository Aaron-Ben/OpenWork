import type { AgentStatus } from "@crew/protocol";
import { assertNever } from "./assert";

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
