import cors from "cors";
import express, { type Express } from "express";
import type { ServerContext } from "./context";
import { errorHandler, notFound, requireAgent, requireToken } from "./http";
import { agentRoutes } from "./routes/agent";
import { computerRoutes } from "./routes/computer";
import { desktopRoutes } from "./routes/desktop";

export interface AppOptions extends ServerContext {
  desktopToken: string;
  computerToken: string;
  /** 唯一允许跨域调用 `/desktop/*` 的来源，即界面的 origin。 */
  rendererOrigin: string;
}

/**
 * 构建 Server 的 Express 应用。不读取环境变量，依赖全部由参数传入，测试可以直接调用。
 *
 * `/desktop/*` 只接受 Desktop 凭证，`/computer/*` 只接受 Computer 凭证，`/agent/*` 只接受 Agent 凭证。
 * 接口的路径与类型由 protocol 的契约 `api` 定义。
 */
export function createApp(options: AppOptions): Express {
  const app = express();
  app.disable("x-powered-by");

  // CORS 在凭证校验之前：浏览器的预检请求不带凭证。
  app.use(
    "/desktop",
    cors({
      origin: (origin, callback) => callback(null, origin === options.rendererOrigin),
      allowedHeaders: ["Authorization", "Content-Type"],
    }),
    requireToken(options.desktopToken),
  );
  app.use("/computer", requireToken(options.computerToken));
  app.use("/agent", requireAgent(options));
  // 先认证再解析请求体：没有凭证的请求不解析。消息正文最多 20,000 字符，1 MB 足够。
  app.use(express.json({ limit: "1mb" }));

  desktopRoutes(app, options);
  computerRoutes(app, options);
  agentRoutes(app, options);

  app.use(notFound);
  app.use(errorHandler);
  return app;
}
