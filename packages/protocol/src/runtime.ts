import { z } from "zod";

/** Desktop 每次启动生成的运行期身份。Server、Computer 与它们签发的凭证都绑定在它上面。 */
export const RuntimeSessionId = z.string().min(1).brand<"RuntimeSessionId">();
export type RuntimeSessionId = z.infer<typeof RuntimeSessionId>;

// 主进程与子进程之间的启动协议：主进程向子进程的 stdin 写一行 JSON（bootstrap），
// 子进程就绪后向 stdout 写一行 JSON（ready）。凭证只经 stdin 传递，不出现在命令行参数与环境变量中。

export const ServerBootstrap = z.object({
  runtimeSessionId: RuntimeSessionId,
  desktopToken: z.string().min(1),
  computerToken: z.string().min(1),
});
export type ServerBootstrap = z.infer<typeof ServerBootstrap>;

export const ServerReady = z.object({
  runtimeSessionId: RuntimeSessionId,
  /** Server 监听的地址，端口由操作系统分配，例如 `http://127.0.0.1:53817`。 */
  baseUrl: z.url(),
});
export type ServerReady = z.infer<typeof ServerReady>;

export const ComputerBootstrap = z.object({
  runtimeSessionId: RuntimeSessionId,
  baseUrl: z.url(),
  computerToken: z.string().min(1),
});
export type ComputerBootstrap = z.infer<typeof ComputerBootstrap>;

export const ComputerReady = z.object({
  runtimeSessionId: RuntimeSessionId,
});
export type ComputerReady = z.infer<typeof ComputerReady>;
