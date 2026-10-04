import type { AppType } from "@crew/server";
import { hc } from "hono/client";

export type ServerClient = ReturnType<typeof hc<AppType>>;

/** 创建调用 Server 的有类型客户端。每个请求都带上 Desktop 凭证。 */
export function createServerClient(serverUrl: string, desktopToken: string, fetchFn?: typeof fetch): ServerClient {
  return hc<AppType>(serverUrl, {
    headers: { Authorization: `Bearer ${desktopToken}` },
    ...(fetchFn ? { fetch: fetchFn } : {}),
  });
}

export type ConnectionStatus = { kind: "connected"; computerConnected: boolean } | { kind: "failed"; reason: string };

/** 读取 Server 报告的连接状态。请求失败与非 2xx 响应都转成带原因的 `failed`。 */
export async function loadStatus(client: ServerClient): Promise<ConnectionStatus> {
  try {
    const response = await client.desktop.status.$get();
    if (!response.ok) {
      return { kind: "failed", reason: `Server 返回 ${response.status}` };
    }
    const { computerConnected } = await response.json();
    return { kind: "connected", computerConnected };
  } catch (error) {
    return { kind: "failed", reason: error instanceof Error ? error.message : String(error) };
  }
}
