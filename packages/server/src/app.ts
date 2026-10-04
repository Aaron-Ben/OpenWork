import { Hono } from "hono";
import { cors } from "hono/cors";
import { HTTPException } from "hono/http-exception";
import type { ServerContext } from "./context";
import { RequestError } from "./errors";
import { agentRoutes } from "./routes/agent";
import { computerRoutes } from "./routes/computer";
import { desktopRoutes } from "./routes/desktop";

export interface AppOptions extends ServerContext {
  desktopToken: string;
  computerToken: string;
  /** 唯一允许跨域调用 `/desktop/*` 的来源，即渲染进程的 origin。 */
  rendererOrigin: string;
}

/**
 * 构建 Server 的 Hono 应用。不读取环境变量，依赖全部由参数传入，测试可以直接调用。
 *
 * `/desktop/*` 只接受 Desktop 凭证，`/computer/*` 只接受 Computer 凭证，`/agent/*` 只接受 Agent 凭证。
 */
export function createApp(options: AppOptions) {
  return new Hono()
    .use(
      "/desktop/*",
      cors({
        origin: (origin) => (origin === options.rendererOrigin ? origin : null),
        allowHeaders: ["Authorization", "Content-Type"],
      }),
    )
    .route("/desktop", desktopRoutes(options, options.desktopToken))
    .route("/computer", computerRoutes(options, options.computerToken))
    .route("/agent", agentRoutes(options))
    .onError((error, c) => {
      if (error instanceof RequestError) return c.json({ error: error.message }, error.status);
      if (error instanceof HTTPException) return error.getResponse();
      console.error("[server] 请求处理失败:", error);
      return c.json({ error: "Server 内部错误" }, 500);
    });
}

/** 渲染进程用它创建有类型的 `hono/client`。 */
export type AppType = ReturnType<typeof createApp>;
