import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { createTestDatabase, testEnv } from "@crew/server/testing";

// 给界面截图，让 Agent 也能看到改动后的样子：用临时数据库启动开发模式的应用，经 DevTools 协议截图，
// 然后关闭应用、删除临时数据库。系统的红黄绿按钮不在截图里，它由 macOS 绘制。
//
// 用法：pnpm preview:shot [--out 文件] [--theme light|dark] [--eval 脚本]
// --eval 在截图前于页面中执行一段 JavaScript，例如点开“新建 agent”对话框；它的返回值会打印出来。

const desktop = fileURLToPath(new URL("..", import.meta.url));
const DEBUG_PORT = 9334;

const { values } = parseArgs({
  options: {
    out: { type: "string", default: join(desktop, "out/preview/shot.png") },
    theme: { type: "string" },
    eval: { type: "string" },
  },
});
if (values.theme !== undefined && values.theme !== "light" && values.theme !== "dark") {
  throw new Error("--theme 只能是 light 或 dark");
}

/** 一个页面的 DevTools 连接：发命令、等回复。 */
class Page {
  private nextId = 0;
  private readonly pending = new Map<number, (message: { result?: unknown; error?: unknown }) => void>();

  private constructor(private readonly socket: WebSocket) {
    socket.addEventListener("message", (event) => {
      const message = JSON.parse(String(event.data)) as { id?: number; result?: unknown; error?: unknown };
      if (message.id !== undefined) this.pending.get(message.id)?.(message);
    });
  }

  static async connect(url: string): Promise<Page> {
    const socket = new WebSocket(url);
    await new Promise((resolveOpen, rejectOpen) => {
      socket.addEventListener("open", resolveOpen, { once: true });
      socket.addEventListener("error", rejectOpen, { once: true });
    });
    return new Page(socket);
  }

  send(method: string, params: Record<string, unknown> = {}): Promise<Record<string, unknown>> {
    const id = ++this.nextId;
    this.socket.send(JSON.stringify({ id, method, params }));
    return new Promise((resolveReply, rejectReply) => {
      this.pending.set(id, (message) => {
        this.pending.delete(id);
        if (message.error) rejectReply(new Error(`${method}：${JSON.stringify(message.error)}`));
        else resolveReply((message.result ?? {}) as Record<string, unknown>);
      });
    });
  }

  async evaluate(expression: string): Promise<unknown> {
    const reply = await this.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
    if (reply.exceptionDetails) throw new Error(`页面脚本出错：${JSON.stringify(reply.exceptionDetails)}`);
    return (reply.result as { value?: unknown } | undefined)?.value;
  }

  close(): void {
    this.socket.close();
  }
}

async function until<T>(read: () => Promise<T | undefined>, timeoutMs: number, what: string): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const value = await read().catch(() => undefined);
    if (value !== undefined) return value;
    await new Promise((resolveWait) => setTimeout(resolveWait, 250));
  }
  throw new Error(`等待超时：${what}`);
}

/** 应用窗口里界面页面的 DevTools 地址。 */
async function pageSocketUrl(): Promise<string> {
  const targets = (await (await fetch(`http://127.0.0.1:${DEBUG_PORT}/json`)).json()) as Array<{
    type: string;
    url: string;
    webSocketDebuggerUrl: string;
  }>;
  const page = targets.find((target) => target.type === "page" && target.url.startsWith("http://localhost"));
  if (!page) throw new Error("应用窗口还没有打开");
  return page.webSocketDebuggerUrl;
}

/**
 * 关闭应用：只给 Electron 主进程发 SIGTERM，让它按正常流程先停 Computer、再停 Server，
 * Computer 因此能删除本次运行的目录。直接结束整个进程组会让 Computer 来不及清理。
 * 最后等 electron-vite 退出；超时就结束整个进程组。
 */
async function stop(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null || child.pid === undefined) return;
  const exited = new Promise((resolveExit) => child.once("exit", resolveExit));
  const electron = execFileSync("/usr/bin/pgrep", ["-P", String(child.pid), "-f", "Electron"], { encoding: "utf8" })
    .split("\n")
    .filter(Boolean);
  for (const pid of electron) process.kill(Number(pid), "SIGTERM");
  const timer = setTimeout(() => {
    if (child.pid !== undefined) process.kill(-child.pid, "SIGKILL");
  }, 10_000);
  await exited;
  clearTimeout(timer);
}

const { redisUrl } = testEnv();
const database = await createTestDatabase();
let app: ChildProcess | undefined;
let appOutput = "";
try {
  app = spawn(join(desktop, "node_modules/.bin/electron-vite"), ["dev", "--remoteDebuggingPort", String(DEBUG_PORT)], {
    cwd: desktop,
    // 主进程读 .env 时不覆盖已有的环境变量，所以这里的临时数据库优先。
    env: { ...process.env, DATABASE_URL: database.url, REDIS_URL: redisUrl },
    detached: true,
    stdio: ["ignore", "ignore", "pipe"],
  });
  // 应用的输出里有构建警告与结束进程时 Chromium 的报错，平时只是噪音；只在截图失败时打印，方便排查。
  app.stderr?.setEncoding("utf8");
  app.stderr?.on("data", (chunk: string) => {
    appOutput += chunk;
  });

  const page = await Page.connect(await until(pageSocketUrl, 60_000, "应用窗口打开"));
  try {
    // 等界面经 SSE 连上 Server，侧栏底部显示“已连接”。
    await until(
      async () => ((await page.evaluate("document.body.innerText.includes('已连接')")) ? true : undefined),
      30_000,
      "界面连上 Server",
    );
    if (values.theme) {
      await page.send("Emulation.setEmulatedMedia", {
        features: [{ name: "prefers-color-scheme", value: values.theme }],
      });
    }
    if (values.eval) {
      // 脚本的返回值打印出来，方便检查页面状态，而不只是看截图。
      const result = await page.evaluate(values.eval);
      if (result !== undefined) console.log(JSON.stringify(result, null, 2));
    }
    // 等主题切换与脚本触发的渲染完成。
    await new Promise((resolveWait) => setTimeout(resolveWait, 500));
    const shot = await page.send("Page.captureScreenshot", { format: "png" });
    // pnpm 在包目录里运行脚本，INIT_CWD 是用户运行命令时所在的目录，相对路径以它为基准。
    const out = resolve(process.env.INIT_CWD ?? process.cwd(), values.out);
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, Buffer.from(String(shot.data), "base64"));
    console.log(`截图已保存：${out}`);
  } finally {
    page.close();
  }
} catch (error) {
  process.stderr.write(`应用输出：\n${appOutput.slice(-4_000)}\n`);
  throw error;
} finally {
  if (app) await stop(app);
  await database.drop();
}
