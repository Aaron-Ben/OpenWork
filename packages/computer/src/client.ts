import { type AgentId, type AgentStatus, ComputerAgent, InboxRoom, type Parser, type RoomId } from "@crew/protocol";
import { z } from "zod";

/**
 * Computer 调用 Server 的客户端。每个响应都按 protocol 中的 schema 校验：Computer 与 Server 是两个进程。
 * 失败时抛出的错误说明原因，供日志与主进程显示。
 */
export class ServerClient {
  constructor(
    readonly baseUrl: string,
    private readonly computerToken: string,
    private readonly fetchFn: typeof fetch = fetch,
  ) {}

  /** 用 Computer 凭证调用 Server 一次，证明地址与凭证有效。 */
  async connect(): Promise<void> {
    await this.request("POST", "/computer/connect");
  }

  async listAgents(): Promise<ComputerAgent[]> {
    return this.request("GET", "/computer/agents", undefined, z.array(ComputerAgent));
  }

  async readInbox(agentId: AgentId): Promise<InboxRoom[]> {
    return this.request("GET", `/computer/agents/${agentId}/inbox`, undefined, z.array(InboxRoom));
  }

  async acknowledge(agentId: AgentId, acks: Array<{ roomId: RoomId; seq: number }>): Promise<void> {
    await this.request("POST", `/computer/agents/${agentId}/inbox/ack`, { acks });
  }

  async issueAgentToken(agentId: AgentId): Promise<string> {
    const { token } = await this.request(
      "POST",
      `/computer/agents/${agentId}/token`,
      undefined,
      z.object({ token: z.string().min(1) }),
    );
    return token;
  }

  async reportStatus(agentId: AgentId, status: AgentStatus): Promise<void> {
    await this.request("POST", `/computer/agents/${agentId}/status`, status);
  }

  async reportModels(models: string[]): Promise<void> {
    await this.request("POST", "/computer/models", { models });
  }

  /** SSE 接口的地址与请求头，交给 `runEventStream`。 */
  eventStream(): { url: string; headers: Record<string, string> } {
    return {
      url: new URL("/computer/events", this.baseUrl).toString(),
      headers: { Authorization: `Bearer ${this.computerToken}` },
    };
  }

  private async request(method: string, path: string, body?: unknown): Promise<undefined>;
  private async request<T>(method: string, path: string, body: unknown, schema: Parser<T>): Promise<T>;
  private async request<T>(method: string, path: string, body?: unknown, schema?: Parser<T>): Promise<T | undefined> {
    let response: Response;
    try {
      response = await this.fetchFn(new URL(path, this.baseUrl), {
        method,
        headers: {
          Authorization: `Bearer ${this.computerToken}`,
          ...(body === undefined ? {} : { "Content-Type": "application/json" }),
        },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch (error) {
      throw new Error(`无法连接 Server（${this.baseUrl}）`, { cause: error });
    }
    if (response.status === 401) throw new Error("Server 拒绝了 Computer 凭证（401）");
    if (!response.ok) {
      const reason = await response.json().then(
        (json: unknown) => (typeof json === "object" && json && "error" in json ? String(json.error) : undefined),
        () => undefined,
      );
      throw new Error(`Server 返回 ${response.status}${reason ? `：${reason}` : ""}（${method} ${path}）`);
    }
    return schema ? schema.parse(await response.json()) : undefined;
  }
}
