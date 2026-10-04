import type { UserId } from "@crew/protocol";
import type { Database } from "./db";
import type { EventHub } from "./events";
import type { PostResult } from "./messages";
import type { RuntimeState } from "./state";

/** 路由需要的全部依赖。由进程入口创建，测试可以自己组装。 */
export interface ServerContext {
  db: Database;
  localUserId: UserId;
  state: RuntimeState;
  events: EventHub;
}

/** 消息写入并提交之后：通知界面这个房间有新消息，唤醒 `wakeTargets` 选出的 Agent。 */
export function notifyMessage(ctx: ServerContext, result: Extract<PostResult, { kind: "posted" }>): void {
  ctx.events.desktop.publish({ type: "room.messages", roomId: result.message.roomId });
  for (const agentId of result.wakeAgentIds) {
    ctx.events.computer.publish({ type: "agent.wake", agentId });
  }
}
