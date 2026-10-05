import { type AgentId, type AgentStatus, api, EVENT_STREAMS } from "@crew/protocol";
import type { Express } from "express";
import { type AgentSummary, createAgent, listAgents } from "../agents";
import { notifyMessage, type ServerContext } from "../context";
import { type Conversation, listConversations, markRead } from "../conversations";
import { addGroupMembers, createGroup, type GroupSummary, listGroups } from "../groups";
import { eventStream, route } from "../http";
import { listMessages, postMessage } from "../messages";
import { agentStatuses, getRun, listRuns } from "../runs";

const IDLE: AgentStatus = { state: "idle" };

/** 界面调用的接口。凭证与 CORS 在 app.ts 中按路径前缀统一处理。 */
export function desktopRoutes(app: Express, ctx: ServerContext): void {
  const view = (agent: AgentSummary, statuses: Map<AgentId, AgentStatus>) => ({
    ...agent,
    createdAt: agent.createdAt.toISOString(),
    status: statuses.get(agent.id) ?? IDLE,
  });
  const statuses = () => agentStatuses(ctx.db, ctx.state.agentProblems());

  const groupView = (group: GroupSummary) => ({ ...group, createdAt: group.createdAt.toISOString() });

  route(app, api.desktop.listAgents, async () => {
    const current = await statuses();
    return (await listAgents(ctx.db)).map((agent) => view(agent, current));
  });

  route(app, api.desktop.createAgent, async ({ body }) => {
    const agent = await createAgent(ctx.db, ctx.localUserId, body);
    ctx.events.desktop.publish({ type: "agents" });
    ctx.events.computer.publish({ type: "agents" });
    return view(agent, new Map());
  });

  route(app, api.desktop.listMessages, ({ params, query }) => listMessages(ctx.db, params.roomId, query));

  route(app, api.desktop.sendMessage, async ({ params, body }) => {
    const result = await postMessage(ctx.db, params.roomId, { kind: "user", id: ctx.localUserId }, body.body);
    // 只有 Agent 的回复会被 HELD 拦下。
    if (result.kind !== "posted") throw new Error("用户的消息被拦下");
    notifyMessage(ctx, result);
    return result.message;
  });

  const conversationView = (conversation: Conversation) => ({
    ...conversation,
    activeAt: conversation.activeAt.toISOString(),
    lastMessage: conversation.lastMessage && {
      ...conversation.lastMessage,
      createdAt: conversation.lastMessage.createdAt.toISOString(),
    },
  });

  route(app, api.desktop.listConversations, async () =>
    (await listConversations(ctx.db, ctx.localUserId)).map(conversationView),
  );

  route(app, api.desktop.markRead, ({ params, body }) => markRead(ctx.db, ctx.localUserId, params.roomId, body.seq));

  route(app, api.desktop.listRuns, ({ query }) => listRuns(ctx.db, query));

  route(app, api.desktop.getRun, ({ params }) => getRun(ctx.db, params.runId));

  route(app, api.desktop.listGroups, async () => (await listGroups(ctx.db)).map(groupView));

  route(app, api.desktop.createGroup, async ({ body }) => {
    const group = await createGroup(ctx.db, ctx.localUserId, body);
    ctx.events.desktop.publish({ type: "rooms" });
    return groupView(group);
  });

  route(app, api.desktop.addGroupMembers, async ({ params, body }) => {
    const group = await addGroupMembers(ctx.db, params.roomId, body.agentIds);
    ctx.events.desktop.publish({ type: "rooms" });
    return groupView(group);
  });

  route(app, api.desktop.listModels, () => ctx.state.listModels());

  app.get(EVENT_STREAMS.desktop, (request, response) => eventStream(request, response, ctx.events.desktop));
}
