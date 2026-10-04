import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { encodeMessage, type Parser, readMessage } from "@crew/protocol";

/** 子进程报告 ready 的最长等待时间。超时按启动失败处理。 */
export const READY_TIMEOUT_MS = 30_000;
/** 停止子进程时等待 SIGTERM 生效的时间，超时后发 SIGKILL。 */
export const STOP_GRACE_MS = 3_000;
/** 保留的 stderr 末尾长度，用于错误提示。 */
const STDERR_TAIL_CHARS = 8_000;

export interface ChildOptions<Ready> {
  /** 显示在日志与错误提示中的名字，例如 "Server"。 */
  name: string;
  /** 运行入口 JS 的可执行文件。生产中是 Electron 自身（配合 ELECTRON_RUN_AS_NODE），测试中是 node。 */
  executable: string;
  entry: string;
  env: NodeJS.ProcessEnv;
  bootstrap: unknown;
  readySchema: Parser<Ready>;
  readyTimeoutMs?: number;
  stopGraceMs?: number;
}

type ExitListener = (code: number | null, signal: NodeJS.Signals | null) => void;

export interface Child<Ready> {
  name: string;
  pid: number;
  ready: Ready;
  /** 最近的 stderr 输出，用于错误提示。 */
  stderrTail(): string;
  /** 子进程在 stop() 之外退出时调用。 */
  onUnexpectedExit(listener: ExitListener): void;
  /** 先发 SIGTERM，宽限期后仍未退出就发 SIGKILL；子进程退出后才返回。 */
  stop(): Promise<void>;
}

/** 子进程没能报告 ready。`stderr` 是它最后的错误输出。 */
export class ChildStartError extends Error {
  constructor(
    message: string,
    readonly stderr: string,
  ) {
    super(message);
  }
}

/**
 * 启动子进程，经 stdin 写入 bootstrap，等待它在 stdout 写出 ready。
 *
 * 子进程在 ready 之前退出、ready 不符合 schema 或超时，都按启动失败处理：
 * 杀掉子进程，抛出带 stderr 末尾的 ChildStartError。
 */
export async function startChild<Ready>(options: ChildOptions<Ready>): Promise<Child<Ready>> {
  const child = spawn(options.executable, [options.entry], {
    env: options.env,
    stdio: ["pipe", "pipe", "pipe"],
  });

  let stderr = "";
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk: string) => {
    stderr = (stderr + chunk).slice(-STDERR_TAIL_CHARS);
    process.stderr.write(prefixLines(options.name, chunk));
  });

  let stopping = false;
  const exited = new Promise<void>((resolve) => child.once("exit", () => resolve()));
  const unexpectedExitListeners: ExitListener[] = [];

  child.stdin.write(encodeMessage(options.bootstrap));

  let ready: Ready;
  try {
    ready = await waitForReady(child, options);
  } catch (error) {
    child.kill("SIGKILL");
    await exited;
    const reason = error instanceof Error ? error.message : String(error);
    throw new ChildStartError(`${options.name} 启动失败：${reason}`, stderr);
  }

  // 记下意外退出：退出之后才注册的监听也要收到，否则这次崩溃就丢了。
  let unexpectedExit: { code: number | null; signal: NodeJS.Signals | null } | undefined;
  child.once("exit", (code, signal) => {
    if (stopping) return;
    unexpectedExit = { code, signal };
    for (const listener of unexpectedExitListeners) listener(code, signal);
  });

  return {
    name: options.name,
    pid: child.pid ?? -1,
    ready,
    stderrTail: () => stderr,
    onUnexpectedExit: (listener) => {
      if (unexpectedExit) listener(unexpectedExit.code, unexpectedExit.signal);
      else unexpectedExitListeners.push(listener);
    },
    stop: async () => {
      stopping = true;
      if (child.exitCode !== null || child.signalCode !== null) return;
      child.kill("SIGTERM");
      const forceKill = setTimeout(() => child.kill("SIGKILL"), options.stopGraceMs ?? STOP_GRACE_MS);
      await exited;
      clearTimeout(forceKill);
    },
  };
}

/** 子进程的 stdout 关闭后，等待它的 exit 事件的时间。 */
const EXIT_AFTER_EOF_MS = 500;

interface ExitInfo {
  code: number | null;
  signal: NodeJS.Signals | null;
}

function exitedBeforeReady({ code, signal }: ExitInfo): Error {
  return new Error(`在 ready 之前退出（code=${code}，signal=${signal}）`);
}

async function waitForReady<Ready>(
  child: ChildProcessWithoutNullStreams,
  options: ChildOptions<Ready>,
): Promise<Ready> {
  const timeoutMs = options.readyTimeoutMs ?? READY_TIMEOUT_MS;
  let timer: NodeJS.Timeout | undefined;
  const timedOut = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${timeoutMs / 1000} 秒内没有报告 ready`)), timeoutMs);
  });
  const exit = new Promise<ExitInfo>((resolve) => child.once("exit", (code, signal) => resolve({ code, signal })));
  try {
    const ready = readMessage(createInterface({ input: child.stdout }), options.readySchema);
    return await Promise.race([
      ready,
      timedOut,
      exit.then((info) => {
        throw exitedBeforeReady(info);
      }),
    ]);
  } catch (error) {
    // 子进程退出时 stdout 先关闭，exit 事件稍后才到。退出码比“输入结束”更能说明原因，所以优先报告它。
    const info = await Promise.race([exit, new Promise<undefined>((r) => setTimeout(r, EXIT_AFTER_EOF_MS))]);
    throw info ? exitedBeforeReady(info) : error;
  } finally {
    clearTimeout(timer);
  }
}

/** 给子进程的每一行输出加上名字前缀，转发到主进程的 stderr。 */
function prefixLines(name: string, chunk: string): string {
  const lines = chunk.split("\n");
  if (lines.at(-1) === "") lines.pop();
  return lines.map((line) => `[${name}] ${line}\n`).join("");
}
