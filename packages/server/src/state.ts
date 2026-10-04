import { randomBytes } from "node:crypto";
import type { AgentId, AgentStatus } from "@crew/protocol";

/**
 * 只存在 Server 内存中的运行期状态。Server 退出时整组进程与 RuntimeSession 一起替换，
 * 这些状态随之清空：Agent 凭证失效，状态回到空闲，模型列表等 Computer 重新上报。
 */
export class RuntimeState {
  /** 本 Server 进程启动以来，Computer 至少连接过一次。不表示 Computer 现在仍在运行。 */
  computerConnected = false;

  private readonly agentByToken = new Map<string, AgentId>();
  private readonly tokenByAgent = new Map<AgentId, string>();
  private readonly statuses = new Map<AgentId, AgentStatus>();
  private models: string[] = [];

  /** 为 Agent 签发一个新凭证。旧凭证随即失效。 */
  issueAgentToken(agentId: AgentId): string {
    const previous = this.tokenByAgent.get(agentId);
    if (previous) this.agentByToken.delete(previous);
    const token = randomBytes(32).toString("base64url");
    this.agentByToken.set(token, agentId);
    this.tokenByAgent.set(agentId, token);
    return token;
  }

  agentForToken(token: string): AgentId | undefined {
    return this.agentByToken.get(token);
  }

  statusOf(agentId: AgentId): AgentStatus {
    return this.statuses.get(agentId) ?? { state: "idle" };
  }

  setStatus(agentId: AgentId, status: AgentStatus): void {
    this.statuses.set(agentId, status);
  }

  listModels(): string[] {
    return this.models;
  }

  setModels(models: string[]): void {
    this.models = [...models];
  }
}
