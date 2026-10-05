import { randomBytes } from "node:crypto";
import type { AgentId } from "@crew/protocol";

/**
 * 只存在 Server 内存中的运行期状态。Server 退出时整组进程与 RuntimeSession 一起替换，
 * 这些状态随之清空：Agent 凭证失效，Agent 跑不起来的原因与模型列表等 Computer 重新上报。
 * Agent 是否在回复、上一轮是否失败，由运行记录推出（`runs.ts` 的 `agentStatuses`），不在这里。
 */
export class RuntimeState {
  private readonly agentByToken = new Map<string, AgentId>();
  private readonly tokenByAgent = new Map<AgentId, string>();
  private readonly problems = new Map<AgentId, string>();
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

  /** Computer 报告的、Agent 跑不起来的原因：沙箱不可用、目录不安全等。 */
  agentProblems(): ReadonlyMap<AgentId, string> {
    return this.problems;
  }

  setProblem(agentId: AgentId, problem: string | null): void {
    if (problem === null) this.problems.delete(agentId);
    else this.problems.set(agentId, problem);
  }

  listModels(): string[] {
    return this.models;
  }

  setModels(models: string[]): void {
    this.models = [...models];
  }
}
