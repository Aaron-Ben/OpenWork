import { execFile } from "node:child_process";
import { mkdtemp, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { ApiClient } from "@crew/protocol";
import { createTestDatabase, testEnv } from "@crew/server/testing";
import { type Runtime, startRuntime } from "../../electron/runtime";

// 冒烟测试与真实模型 e2e 共用：构建应用，再用主进程同一份 startRuntime 启动构建好的 Server 与 Computer。

const desktop = fileURLToPath(new URL("../..", import.meta.url));
export const electronPath = join(desktop, "node_modules/electron/dist/Electron.app/Contents/MacOS/Electron");

export interface BuiltApp {
  runtime: Runtime;
  client: ApiClient;
  /** 停止 Server 与 Computer，删除构建产物、临时数据库与临时目录。 */
  close(): Promise<void>;
}

/**
 * 构建应用并启动它。构建产物、crew 目录与数据库都是本次运行私有的临时资源，
 * 同时运行的两次检查互不影响。`env` 覆盖传给 Server 与 Computer 的环境变量，参数是本次的临时目录。
 */
export async function startBuiltApp(
  env: (root: string) => Promise<NodeJS.ProcessEnv> | NodeJS.ProcessEnv,
): Promise<BuiltApp> {
  const { redisUrl } = testEnv();
  const root = await realpath(await mkdtemp(join(tmpdir(), "crew-built-")));
  const removeRoot = () => rm(root, { recursive: true, force: true });

  const outDir = join(root, "out");
  try {
    await promisify(execFile)(
      join(desktop, "node_modules/.bin/electron-vite"),
      ["build", "--mode", "smoke", "--outDir", outDir],
      { cwd: desktop },
    );
  } catch (error) {
    await removeRoot();
    throw error;
  }

  const database = await createTestDatabase().catch(async (error: unknown) => {
    await removeRoot();
    throw error;
  });
  const cleanup = async () => {
    await database.drop();
    await removeRoot();
  };

  try {
    const runtime = await startRuntime({
      executable: await realpath(electronPath),
      serverEntry: join(outDir, "main/server.js"),
      computerEntry: join(outDir, "main/computer.js"),
      migrationsDir: join(outDir, "main/drizzle"),
      shimEntry: join(outDir, "main/shim.js"),
      crewRoot: join(root, "crew"),
      env: { ...process.env, DATABASE_URL: database.url, REDIS_URL: redisUrl, ...(await env(root)) },
      rendererOrigin: "http://localhost:5173",
    });
    return {
      runtime,
      client: new ApiClient({ baseUrl: runtime.serverUrl, token: runtime.desktopToken }),
      close: async () => {
        await runtime.stop();
        await cleanup();
      },
    };
  } catch (error) {
    await cleanup();
    throw error;
  }
}

/** 轮询直到 `read` 返回值；超时只是等待的上限，不是断言成立的条件。 */
export async function until<T>(read: () => Promise<T | undefined>, timeoutMs: number): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const value = await read();
    if (value !== undefined) return value;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`等待超时（${timeoutMs} 毫秒）`);
}
