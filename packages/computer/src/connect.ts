import { ServerClient } from "./client";

/**
 * 用 Computer 凭证调用 Server 一次，证明地址与凭证有效。
 *
 * 在 loopback 上失败只可能是 Server 没有启动、凭证错误或协议不匹配，重试不会恢复，
 * 所以失败时直接抛出，由进程入口退出。错误信息说明具体原因，供主进程显示。
 *
 * @param fetchFn 测试传入内存中的 Server；生产使用全局 `fetch`。
 */
export async function connectToServer(
  baseUrl: string,
  computerToken: string,
  fetchFn: typeof fetch = fetch,
): Promise<void> {
  await new ServerClient(baseUrl, computerToken, fetchFn).connect();
}
