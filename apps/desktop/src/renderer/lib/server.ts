import type { AppType } from "@crew/server";
import { type ClientResponse, DetailedError, hc, type InferResponseType, parseResponse } from "hono/client";

export type ServerClient = ReturnType<typeof hc<AppType>>;

/** 创建调用 Server 的有类型客户端。每个请求都带上 Desktop 凭证。 */
export function createServerClient(serverUrl: string, desktopToken: string, fetchFn?: typeof fetch): ServerClient {
  return hc<AppType>(serverUrl, {
    headers: { Authorization: `Bearer ${desktopToken}` },
    ...(fetchFn ? { fetch: fetchFn } : {}),
  });
}

export type Agent = InferResponseType<ServerClient["desktop"]["agents"]["$get"], 200>[number];
export type Message = InferResponseType<ServerClient["desktop"]["rooms"][":roomId"]["messages"]["$get"], 200>[number];

/** 从 Server 的错误响应体 `{ error }` 里取出原因；格式不对时用状态码说明。 */
export function errorMessage(body: unknown, status: number): string {
  if (typeof body === "object" && body !== null && "error" in body && typeof body.error === "string") {
    return body.error;
  }
  return `Server 返回 ${status}`;
}

/**
 * 发出请求并返回成功响应的数据，类型由 `hono/client` 推断。
 * 非 2xx 时抛出带 Server 原因的错误，交给 TanStack Query 与表单显示。
 */
export async function request<T extends ClientResponse<unknown>>(
  response: Promise<T>,
): ReturnType<typeof parseResponse<T>> {
  try {
    return await parseResponse(response);
  } catch (error) {
    if (error instanceof DetailedError) throw new Error(errorMessage(error.detail?.data, error.statusCode));
    throw error;
  }
}
