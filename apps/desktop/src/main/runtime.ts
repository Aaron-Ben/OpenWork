import { randomBytes, randomUUID } from "node:crypto";
import { ComputerReady, RuntimeSessionId, ServerReady } from "@crew/protocol";
import { type Child, startChild } from "./child";

/** Computer 只经 Server 的 HTTP 接口访问数据，启动它时删除这些数据库相关的环境变量。 */
const DATABASE_ENV_NAMES = [
  "DATABASE_URL",
  "REDIS_URL",
  "TEST_DATABASE_URL",
  "TEST_REDIS_URL",
  "PGHOST",
  "PGHOSTADDR",
  "PGPORT",
  "PGDATABASE",
  "PGUSER",
  "PGPASSWORD",
  "PGPASSFILE",
  "PGSERVICE",
  "PGSERVICEFILE",
];

export function computerEnv(env: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const result = { ...env };
  for (const name of DATABASE_ENV_NAMES) delete result[name];
  return result;
}

export interface RuntimeOptions {
  /** 运行入口 JS 的可执行文件，生产中是 Electron 自身。 */
  executable: string;
  serverEntry: string;
  computerEntry: string;
  /** drizzle-kit 生成的迁移目录，Server 启动时执行其中的迁移。 */
  migrationsDir: string;
  /** 打包后的 shim 入口，Computer 用它生成 `bin/crew`。 */
  shimEntry: string;
  /** Computer 的根目录，通常是 `~/.crew`。 */
  crewRoot: string;
  /** 已经读入 `.env` 的环境变量，原样传给 Server。 */
  env: NodeJS.ProcessEnv;
  /** 渲染进程的 origin，Server 只允许它跨域调用。 */
  rendererOrigin: string;
}

export interface Runtime {
  runtimeSessionId: RuntimeSessionId;
  serverUrl: string;
  desktopToken: string;
  /** Server 或 Computer 在 stop() 之外退出时调用，参数是退出的那个进程。 */
  onCrash(listener: (crashed: Child<unknown>) => void): void;
  /** 先停 Computer，再停 Server。 */
  stop(): Promise<void>;
}

function newToken(): string {
  return randomBytes(32).toString("base64url");
}

/**
 * 生成一个 RuntimeSession 与它的凭证，依次启动 Server 与 Computer，两者都 ready 后返回。
 *
 * Computer 启动失败时先停掉已经启动的 Server，再抛出错误。
 */
export async function startRuntime(options: RuntimeOptions): Promise<Runtime> {
  const runtimeSessionId = RuntimeSessionId.parse(randomUUID());
  const desktopToken = newToken();
  const computerToken = newToken();
  // Server 与 Computer 是普通的 Node 程序，用 Electron 自带的 Node 运行。
  const nodeEnv = { ELECTRON_RUN_AS_NODE: "1" };

  const server = await startChild({
    name: "Server",
    executable: options.executable,
    entry: options.serverEntry,
    env: {
      ...options.env,
      ...nodeEnv,
      CREW_RENDERER_ORIGIN: options.rendererOrigin,
      CREW_MIGRATIONS_DIR: options.migrationsDir,
    },
    bootstrap: { runtimeSessionId, desktopToken, computerToken },
    readySchema: ServerReady,
  });

  let computer: Child<ComputerReady>;
  try {
    computer = await startChild({
      name: "Computer",
      executable: options.executable,
      entry: options.computerEntry,
      env: { ...computerEnv(options.env), ...nodeEnv },
      bootstrap: {
        runtimeSessionId,
        baseUrl: server.ready.baseUrl,
        computerToken,
        crewRoot: options.crewRoot,
        shimEntry: options.shimEntry,
      },
      readySchema: ComputerReady,
    });
  } catch (error) {
    await server.stop();
    throw error;
  }

  const stop = async () => {
    await computer.stop();
    await server.stop();
  };

  return {
    runtimeSessionId,
    serverUrl: server.ready.baseUrl,
    desktopToken,
    onCrash: (listener) => {
      server.onUnexpectedExit(() => listener(server));
      computer.onUnexpectedExit(() => listener(computer));
    },
    stop,
  };
}
