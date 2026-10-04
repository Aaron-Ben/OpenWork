import type { AgentId, DesktopAgent, DesktopGroup } from "@crew/protocol";

export interface NewGroupErrors {
  name?: string;
  agentIds?: string;
}

/** 提交前检查，文案与 Server 的校验一致。长度由输入框的 maxLength 限制。 */
export function validateNewGroup(input: { name: string; agentIds: readonly AgentId[] }): NewGroupErrors {
  const errors: NewGroupErrors = {};
  if (!input.name.trim()) errors.name = "群聊名字不能为空";
  if (input.agentIds.length === 0) errors.agentIds = "至少选择一个 agent";
  return errors;
}

/** 群聊里的 Agent，按 Agent 列表的顺序。列表里找不到的（还没刷新到）先不显示。 */
export function groupMembers(group: DesktopGroup, agents: readonly DesktopAgent[]): DesktopAgent[] {
  const ids = new Set(group.agentIds);
  return agents.filter((agent) => ids.has(agent.id));
}

/** 还不在群聊里、可以加进来的 Agent。 */
export function nonMembers(group: DesktopGroup, agents: readonly DesktopAgent[]): DesktopAgent[] {
  const ids = new Set(group.agentIds);
  return agents.filter((agent) => !ids.has(agent.id));
}

/** 勾选或取消一个 Agent。 */
export function toggle(ids: readonly AgentId[], id: AgentId): AgentId[] {
  return ids.includes(id) ? ids.filter((other) => other !== id) : [...ids, id];
}
