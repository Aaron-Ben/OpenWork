import { Hono } from "hono";
import { bearerAuth } from "hono/bearer-auth";
import { cors } from "hono/cors";

export interface AppOptions {
  desktopToken: string;
  computerToken: string;
  /** 唯一允许跨域调用 `/desktop/*` 的来源，即渲染进程的 origin。 */
  rendererOrigin: string;
}

/**
 * 构建 Server 的 Hono 应用。不读取环境变量，不访问网络与数据库，测试可以直接调用。
 *
 * `/desktop/*` 只接受 Desktop 凭证，`/computer/*` 只接受 Computer 凭证。
 */
export function createApp(options: AppOptions) {
  // 含义是“本 Server 进程启动以来，Computer 至少连接过一次”，不表示 Computer 现在仍在运行。
  // Server 退出时整组进程与 RuntimeSession 一起替换，新进程从 false 开始。
  let computerConnected = false;

  const desktop = new Hono()
    .use(bearerAuth({ token: options.desktopToken }))
    .get("/status", (c) => c.json({ computerConnected }));

  const computer = new Hono().use(bearerAuth({ token: options.computerToken })).post("/connect", (c) => {
    computerConnected = true;
    return c.body(null, 204);
  });

  return new Hono()
    .use(
      "/desktop/*",
      cors({
        origin: (origin) => (origin === options.rendererOrigin ? origin : null),
        allowHeaders: ["Authorization", "Content-Type"],
      }),
    )
    .route("/desktop", desktop)
    .route("/computer", computer);
}

/** 渲染进程用它创建有类型的 `hono/client`。 */
export type AppType = ReturnType<typeof createApp>;
