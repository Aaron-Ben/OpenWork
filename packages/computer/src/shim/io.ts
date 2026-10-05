import { ErrorBody } from "@crew/protocol";
import type { z } from "zod";

// `crew` 各命令共用的部分：与外界打交道的途径、凭证、请求与可以向 Agent 解释的失败。

/** shim 与外界打交道的全部途径。入口传入真实的 process，测试传入假的。 */
export interface CliIo {
  env: Record<string, string | undefined>;
  readStdin(): Promise<string>;
  readFile(path: string): Promise<string>;
  stdout(text: string): void;
  stderr(text: string): void;
  fetch: typeof fetch;
}

/** 等待 Server 响应的上限。超时后不重试：第 2 步没有幂等，重试可能发出重复消息。 */
export const REQUEST_TIMEOUT_MS = 10_000;

/** 一次可以向 Agent 解释的失败：写一行 `error: …` 到 stderr，退出码 1。 */
export class CliFailure extends Error {}

/** Server 的地址与本 Agent 的凭证，由 Computer 经环境变量与运行期文件提供。 */
export async function credentials(io: CliIo): Promise<{ serverUrl: string; token: string }> {
  const outside = new CliFailure("crew must be run inside a Crew agent turn.");
  const serverUrl = io.env.CREW_SERVER_URL;
  const tokenFile = io.env.CREW_TOKEN_FILE;
  if (!serverUrl || !tokenFile) throw outside;
  let token: string;
  try {
    token = (await io.readFile(tokenFile)).trim();
  } catch {
    // 凭证文件不存在或读不到，原因对 Agent 没有用：它只能知道自己不在一轮 Turn 里。
    throw outside;
  }
  if (!token) throw outside;
  return { serverUrl, token };
}

/**
 * 以本 Agent 的凭证调用 Server 的一个接口。到不了或超时时抛出可以解释的失败，`unsure` 是超时时
 * 说明操作可能已经完成的那半句；其余响应（包括拒绝）原样返回，由调用方解释。
 */
export async function postAgent(io: CliIo, path: string, body: unknown, unsure: string): Promise<Response> {
  const { serverUrl, token } = await credentials(io);
  try {
    return await io.fetch(new URL(path, serverUrl), {
      method: "POST",
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
  } catch (error) {
    if (error instanceof DOMException && error.name === "TimeoutError") {
      throw new CliFailure(`Crew did not answer within ${REQUEST_TIMEOUT_MS / 1000} seconds. ${unsure}`);
    }
    throw new CliFailure(`could not reach Crew (${String(error)}). Nothing was changed.`);
  }
}

/** 拒绝响应的正文：`{ error, refusal? }`。读不出来时返回 undefined。 */
export async function errorBody(response: Response): Promise<z.infer<typeof ErrorBody> | undefined> {
  const parsed = ErrorBody.safeParse(await response.json().catch(() => undefined));
  return parsed.success ? parsed.data : undefined;
}

export function indent(text: string): string {
  return text
    .split("\n")
    .map((line) => `  ${line}`)
    .join("\n");
}
