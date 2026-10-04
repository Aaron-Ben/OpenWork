import { execFile, spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { readFile, realpath, stat } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { stripVTControlCharacters } from "node:util";
import { z } from "zod";
import { buildProfile, classifySandboxExit, SANDBOX_EXEC, sandboxArgs } from "../sandbox";
import type { EngineAdapter, EngineError, EngineReadiness, TurnOutcome, TurnRequest } from "./types";

// OpenCode 适配器：每个 Turn 在 Seatbelt 中启动一次 `opencode run`。
// 选择与验证见设计 Agent Note 的“第 2 步的实现决策 → Engine”。

/** 用户的 OpenCode 登录文件超过这个大小时拒绝启动，不截断。 */
const MAX_AUTH_BYTES = 64 * 1024;
const MAX_STDOUT_BYTES = 8 * 1024 * 1024;
const MAX_STDERR_BYTES = 1024 * 1024;
/** 错误信息只保留 stderr 的末尾。 */
const ERROR_TAIL_CHARS = 16 * 1024;
/** 中止时先发 SIGINT，过这么久仍未退出就发 SIGKILL。 */
const KILL_GRACE_MS = 2_000;

const CANCELLED: TurnOutcome = { ok: false, error: { kind: "cancelled", message: "已停止" } };

/** `opencode run --format json` 输出的一行事件。只取需要的字段，其余忽略。 */
const OpenCodeEvent = z.object({
  type: z.string(),
  sessionID: z.string().optional(),
  error: z
    .object({ name: z.string().optional(), data: z.object({ message: z.string().optional() }).optional() })
    .optional(),
});

export interface OpenCodeOptions {
  /** `opencode` 可执行文件；不给时在 PATH 中查找。 */
  executable?: string;
  /** 用户 OpenCode 数据目录，登录文件在其中的 `opencode/auth.json`。 */
  userDataHome?: string;
}

/** 用户自己的 OpenCode 数据目录：`$XDG_DATA_HOME`，默认 `~/.local/share`。 */
export function defaultUserDataHome(): string {
  return process.env.XDG_DATA_HOME || join(homedir(), ".local", "share");
}

/**
 * 在沙箱外读取用户的登录文件，经 `OPENCODE_AUTH_CONTENT` 传给沙箱内的 OpenCode。
 * OpenCode 在这个变量不是合法 JSON 时会静默改读 `auth.json`，而沙箱禁止它读，所以这里先校验。
 */
export async function readAuthContent(userDataHome: string): Promise<string | EngineError> {
  const file = join(userDataHome, "opencode", "auth.json");
  if (!existsSync(file)) {
    return { kind: "unauthenticated", message: "OpenCode 未登录：在终端运行 opencode auth login" };
  }
  const info = await stat(file);
  if (!info.isFile()) {
    return { kind: "unauthenticated", message: "OpenCode 登录文件不是普通文件：重新运行 opencode auth login" };
  }
  if (info.size > MAX_AUTH_BYTES) {
    return { kind: "unauthenticated", message: `OpenCode 登录文件超过 ${MAX_AUTH_BYTES / 1024} KiB，拒绝启动` };
  }
  const content = await readFile(file, "utf8");
  try {
    JSON.parse(content);
  } catch {
    return { kind: "unauthenticated", message: "OpenCode 登录文件不是合法的 JSON：重新运行 opencode auth login" };
  }
  return content;
}

/**
 * 派生配置：常驻规则、放行全部操作（安全边界是 Seatbelt），并把本次的模型标为 active。
 * OpenCode 刷新模型目录后会删除标为 deprecated 的模型，配置里的状态优先于目录。
 * 理由见 Rust 版的 Agent Note `opencode-models-pinned-active`。
 */
export function derivedConfig(instructionsFile: string, model: string): string {
  const slash = model.indexOf("/");
  const provider = slash > 0 ? model.slice(0, slash) : undefined;
  const modelName = slash > 0 ? model.slice(slash + 1) : undefined;
  return JSON.stringify({
    instructions: [instructionsFile],
    permission: { "*": "allow" },
    ...(provider && modelName ? { provider: { [provider]: { models: { [modelName]: { status: "active" } } } } } : {}),
  });
}

/** 把错误文本里的凭证与 Agent 目录替换掉，再交给界面显示。 */
export function redact(text: string, agentHome: string): string {
  return text
    .replaceAll(agentHome, "<agent-home>")
    .replace(/(Bearer\s+)\S+/gi, "$1<redacted>")
    .replace(/(token=)\S+/gi, "$1<redacted>");
}

/**
 * 根据退出码、stderr 与输出中的 error 事件判断失败原因。
 * “未登录”的服务在 OpenCode 里表现为模型找不到，所以 `ProviderModelNotFoundError` 归为模型不可用。
 */
export function classifyFailure(exitCode: number, stderr: string, reported: string | undefined): EngineError {
  const sandbox = classifySandboxExit(exitCode, stderr);
  if (sandbox.kind === "sandbox-failed") return { kind: "sandbox", message: `沙箱无法启动：${sandbox.message}` };
  if (/Session not found/i.test(stderr)) return { kind: "session-invalid", message: "旧 session 不存在" };

  const detail = lastLogError(stderr) ?? reported ?? lastLine(stderr) ?? `OpenCode 以退出码 ${exitCode} 结束`;
  if (/ProviderModelNotFoundError|Model not found/i.test(stderr)) {
    return {
      kind: "model-unavailable",
      message: `模型不可用：${detail}。确认已用 opencode auth login 登录对应的服务，且模型名正确`,
    };
  }
  if (/\b429\b|rate.?limit/i.test(stderr)) return { kind: "rate-limited", message: detail };
  if (/\b401\b|unauthori[sz]ed|invalid api key/i.test(stderr)) {
    return { kind: "unauthenticated", message: `登录已失效：${detail}。重新运行 opencode auth login` };
  }
  return reported ? { kind: "reported", message: reported } : { kind: "process", message: detail };
}

/** `--print-logs` 输出中最后一条 `error="..."`。 */
function lastLogError(stderr: string): string | undefined {
  const matches = [...stderr.matchAll(/error="([^"]+)"/g)];
  return matches.at(-1)?.[1];
}

function lastLine(text: string): string | undefined {
  const lines = stripVTControlCharacters(text)
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  return lines.at(-1);
}

export class OpenCodeAdapter implements EngineAdapter {
  readonly id = "opencode";

  constructor(private readonly options: OpenCodeOptions = {}) {}

  async probe(): Promise<EngineReadiness> {
    const executable = this.options.executable ?? (await which("opencode"));
    if (!executable) return { ready: false, reason: "找不到 opencode：先安装 OpenCode" };
    return { ready: true, executable: await realpath(executable) };
  }

  async listModels(): Promise<string[]> {
    const readiness = await this.probe();
    if (!readiness.ready) return [];
    const stdout = await new Promise<string>((resolve, reject) => {
      execFile(readiness.executable, ["models"], { timeout: 30_000 }, (error, out) =>
        error ? reject(error) : resolve(out),
      );
    });
    return stdout
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => /^[\w.-]+\/\S+$/.test(line));
  }

  /** 按约定不抛出：任何意外错误都转成失败的结果，Runner 据此上报原因。 */
  async runTurn(request: TurnRequest): Promise<TurnOutcome> {
    try {
      const first = await this.runOnce(request);
      if (!first.ok && first.error.kind === "session-invalid" && request.sessionId) {
        return await this.runOnce({ ...request, sessionId: undefined });
      }
      return first;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      return {
        ok: false,
        error: { kind: "process", message: redact(`OpenCode 无法启动：${message}`, request.layout.home) },
      };
    }
  }

  private async runOnce(request: TurnRequest): Promise<TurnOutcome> {
    const readiness = await this.probe();
    if (!readiness.ready) return { ok: false, error: { kind: "process", message: readiness.reason } };
    const auth = await readAuthContent(this.options.userDataHome ?? defaultUserDataHome());
    if (typeof auth !== "string") return { ok: false, error: auth };

    const { layout } = request;
    const command = [
      readiness.executable,
      "run",
      "--pure",
      "--format",
      "json",
      "--print-logs",
      "--log-level",
      "ERROR",
      "--auto",
      ...(request.sessionId ? ["--session", request.sessionId] : []),
      "--model",
      request.model,
    ];
    const env: Record<string, string> = {
      PATH: process.env.PATH ?? "/usr/bin:/bin",
      LANG: process.env.LANG ?? "en_US.UTF-8",
      ...(process.env.TMPDIR ? { TMPDIR: process.env.TMPDIR } : {}),
      ...request.env,
      HOME: layout.home,
      XDG_DATA_HOME: layout.engineDataDir,
      XDG_CONFIG_HOME: layout.configDir,
      XDG_CACHE_HOME: layout.cacheDir,
      XDG_STATE_HOME: layout.stateDir,
      OPENCODE_DISABLE_PROJECT_CONFIG: "1",
      OPENCODE_AUTH_CONTENT: auth,
      OPENCODE_CONFIG_CONTENT: derivedConfig(layout.instructionsFile, request.model),
    };

    // 准备期间（probe、读登录文件）已经被停止时不再启动：之后注册的 abort 监听不会再触发。
    if (request.signal.aborted) return CANCELLED;
    const child = spawn(SANDBOX_EXEC, sandboxArgs(buildProfile(request.confinement), command), {
      cwd: layout.workDir,
      env,
      detached: true,
      stdio: ["pipe", "pipe", "pipe"],
    });

    let sessionId: string | undefined;
    let reported: string | undefined;
    let stdoutBytes = 0;
    let stderr = "";
    let stderrBytes = 0;
    let overflow = false;
    let pending = "";

    const killGroup = (signal: NodeJS.Signals) => {
      if (child.pid === undefined) return;
      try {
        process.kill(-child.pid, signal);
      } catch (error) {
        // 进程组已经全部退出：没有要结束的进程。
        if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error;
      }
    };
    let forceKill: NodeJS.Timeout | undefined;
    const stop = () => {
      killGroup("SIGINT");
      forceKill = setTimeout(() => killGroup("SIGKILL"), KILL_GRACE_MS);
    };
    request.signal.addEventListener("abort", stop, { once: true });
    // spawn 与注册监听之间被停止时，监听不会触发，这里补上。
    if (request.signal.aborted) stop();

    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => {
      stdoutBytes += Buffer.byteLength(chunk);
      if (stdoutBytes > MAX_STDOUT_BYTES && !overflow) {
        overflow = true;
        stop();
        return;
      }
      pending += chunk;
      const lines = pending.split("\n");
      pending = lines.pop() ?? "";
      for (const line of lines) {
        const parsed = parseEvent(line);
        if (!parsed) continue;
        sessionId = parsed.sessionID ?? sessionId;
        if (parsed.type === "error")
          reported = parsed.error?.data?.message ?? parsed.error?.name ?? "OpenCode 报告了错误";
      }
    });
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string) => {
      stderrBytes += Buffer.byteLength(chunk);
      if (stderrBytes <= MAX_STDERR_BYTES) stderr = (stderr + chunk).slice(-ERROR_TAIL_CHARS);
    });

    // OpenCode 退出后，结束它留在进程组里的全部进程（Agent 用 `cmd &` 起的后台进程等）：
    // 它们在沙箱里能联网，不该活过这一轮；继承了 stdout 的还会让 close 一直不来，这一轮因此卡住。
    let leftoverKill: NodeJS.Timeout | undefined;
    child.once("exit", () => {
      killGroup("SIGTERM");
      leftoverKill = setTimeout(() => killGroup("SIGKILL"), KILL_GRACE_MS);
    });

    const exitCode = await new Promise<number>((resolve) => {
      child.on("error", (error) => {
        stderr += `\n${error.message}`;
        resolve(-1);
      });
      child.on("close", (code) => resolve(code ?? -1));
      child.stdin.on("error", () => {
        // Engine 在读完 prompt 之前退出时写入会失败；退出原因由退出码与 stderr 说明。
      });
      child.stdin.end(request.prompt);
    });
    clearTimeout(forceKill);
    clearTimeout(leftoverKill);
    request.signal.removeEventListener("abort", stop);
    const lastEvent = parseEvent(pending);
    if (lastEvent) sessionId = lastEvent.sessionID ?? sessionId;

    if (request.signal.aborted) return CANCELLED;
    if (overflow) {
      return {
        ok: false,
        error: { kind: "output-limit", message: `OpenCode 输出超过 ${MAX_STDOUT_BYTES / 1024 / 1024} MiB` },
      };
    }
    if (exitCode === 0 && !reported) {
      if (!sessionId) return { ok: false, error: { kind: "process", message: "OpenCode 的输出里没有 session ID" } };
      return { ok: true, sessionId };
    }
    const error = classifyFailure(exitCode, stderr, reported);
    return { ok: false, error: { ...error, message: redact(error.message, layout.home) } };
  }
}

function parseEvent(line: string) {
  if (!line.trim()) return undefined;
  try {
    const result = OpenCodeEvent.safeParse(JSON.parse(line));
    return result.success ? result.data : undefined;
  } catch {
    // 不是 JSON 的行（例如 OpenCode 偶尔打印的提示）忽略，失败原因以退出码与 stderr 为准。
    return undefined;
  }
}

/** 在 PATH 中查找可执行文件。 */
function which(name: string): Promise<string | undefined> {
  return new Promise((resolve) => {
    execFile("/usr/bin/which", [name], { timeout: 3_000 }, (error, stdout) => {
      resolve(error ? undefined : stdout.trim() || undefined);
    });
  });
}
