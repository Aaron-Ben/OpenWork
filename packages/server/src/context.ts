import type { UserId } from "@crew/protocol";
import type { Database } from "./db";
import type { EventHub } from "./events";
import type { PostResult } from "./messages";
import type { RunActivity } from "./runs";
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

/**
 * 一轮有了新的一步或结束了：通知界面刷新这一轮。`statusChanged` 时（开始、结束）Agent 的状态也变了，
 * 一起刷新 Agent 列表。不在任何一轮里时什么也不做。
 */
export function notifyRun(ctx: ServerContext, activity: RunActivity | undefined, statusChanged = false): void {
  if (!activity) return;
  ctx.events.desktop.publish({ type: "run.activity", runId: activity.runId, roomIds: activity.roomIds });
  if (statusChanged) ctx.events.desktop.publish({ type: "agents" });
}
