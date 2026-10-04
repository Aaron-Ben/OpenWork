import {
  type AgentId,
  type AgentStatus,
  ApiClient,
  ApiError,
  api,
  type CallArgs,
  type ComputerAgent,
  type Endpoint,
  EVENT_STREAMS,
  type InboxRoom,
  type ResponseOf,
  type RoomId,
} from "@crew/protocol";
import { ZodError } from "zod";

/**
 * Computer 调用 Server 的客户端。接口与响应 schema 来自 protocol 的契约 `api`，每个响应都经过校验：
 * Computer 与 Server 是两个进程。失败时抛出的错误说明原因，供日志与主进程显示。
 */
export class ServerClient {
  private readonly client: ApiClient;

  constructor(
    readonly baseUrl: string,
    private readonly computerToken: string,
    private readonly fetchFn: typeof fetch = fetch,
  ) {
    this.client = new ApiClient({ baseUrl, token: computerToken, fetch: fetchFn });
  }

  /** 用 Computer 凭证调用 Server 一次，证明地址与凭证有效。 */
  async connect(): Promise<void> {
    await this.call(api.computer.connect);
  }

  listAgents(): Promise<ComputerAgent[]> {
    return this.call(api.computer.listAgents);
  }

  readInbox(agentId: AgentId): Promise<InboxRoom[]> {
    return this.call(api.computer.readInbox, { params: { agentId } });
  }

  async acknowledge(agentId: AgentId, acks: Array<{ roomId: RoomId; seq: number }>): Promise<void> {
    await this.call(api.computer.acknowledge, { params: { agentId }, body: { acks } });
  }

  async issueAgentToken(agentId: AgentId): Promise<string> {
    const { token } = await this.call(api.computer.issueAgentToken, { params: { agentId } });
    return token;
  }

  async reportStatus(agentId: AgentId, status: AgentStatus): Promise<void> {
    await this.call(api.computer.reportStatus, { params: { agentId }, body: status });
  }

  async reportModels(models: string[]): Promise<void> {
    await this.call(api.computer.reportModels, { body: { models } });
  }

  /** SSE 接口的地址、请求头与本客户端使用的 fetch，交给 `runEventStream`。 */
  eventStream(): { url: string; headers: Record<string, string>; fetch: typeof fetch } {
    return {
      url: new URL(EVENT_STREAMS.computer, this.baseUrl).toString(),
      headers: { Authorization: `Bearer ${this.computerToken}` },
      fetch: this.fetchFn,
    };
  }

  /** 调用一个接口，把失败换成说明原因的错误。 */
  private async call<E extends Endpoint>(endpoint: E, ...args: CallArgs<E>): Promise<ResponseOf<E>> {
    try {
      return await this.client.call(endpoint, ...args);
    } catch (error) {
      const where = `${endpoint.method} ${endpoint.path}`;
      if (error instanceof ZodError) throw new Error(`Server 的响应不符合协议（${where}）`, { cause: error });
      if (!(error instanceof ApiError)) throw new Error(`无法连接 Server（${this.baseUrl}）`, { cause: error });
      if (error.status === 401) throw new Error("Server 拒绝了 Computer 凭证（401）");
      throw new Error(`Server 返回 ${error.status}：${error.message}（${where}）`);
    }
  }
}
