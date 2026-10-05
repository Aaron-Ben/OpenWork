import { createHash } from "node:crypto";
import { chmod, lstat, mkdir, readdir, readFile, realpath, rename, rm, stat, writeFile } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { type AgentId, agentWorkSegments, MEMORY_FILE, type RuntimeSessionId } from "@crew/protocol";
import { z } from "zod";
import { type AgentIdentity, standingInstructions } from "./instructions";
import type { Confinement } from "./sandbox";

// Computer 在本机的目录布局。目录权限 0700，文件 0600；写文件先写临时文件再改名，读者不会看到写了一半的内容。
//
// ~/.crew/
// ├── agents/<agent-id>/                 持久：跨 RuntimeSession 保留
// │   ├── AGENTS.md                      身份与规则
// │   ├── work/                          OpenCode 的工作目录
// │   │   └── MEMORY.md                  Agent 的记忆，由它自己维护
// │   └── engines/opencode/
// │       ├── data/                      OpenCode 的数据目录（XDG_DATA_HOME），每个 Agent 独立
// │       └── session.json               上次的 session
// └── runtime/<runtime-session-id>/      短期：只属于本次运行，启动时清理旧的，正常退出时删除
//     ├── bin/crew                       shim 的包装脚本
//     └── agents/<agent-id>/
//         ├── token                      Agent 凭证
//         └── config/ cache/ state/      OpenCode 的配置、缓存与状态目录

/** Computer 的根目录。 */
export function defaultCrewRoot(): string {
  return join(homedir(), ".crew");
}

export interface RuntimeLayout {
  root: string;
  runtimeDir: string;
  binDir: string;
}

export interface AgentLayout {
  home: string;
  workDir: string;
  /** 工作目录里的 `MEMORY.md`。 */
  memoryFile: string;
  instructionsFile: string;
  engineDataDir: string;
  sessionFile: string;
  runtimeDir: string;
  tokenFile: string;
  configDir: string;
  cacheDir: string;
  stateDir: string;
}

async function ensureDir(path: string): Promise<void> {
  await mkdir(path, { recursive: true, mode: 0o700 });
  await chmod(path, 0o700);
}

/** Agent 的目录里出现了符号链接或特殊文件。Computer 不受沙箱约束，不顺着这类路径操作。 */
export class UnsafePathError extends Error {}

function isMissing(error: unknown): boolean {
  return (error as NodeJS.ErrnoException).code === "ENOENT";
}

/**
 * 在 `base` 下逐级建好 `path`，权限 0700。
 *
 * `base` 是 Agent 的持久目录或本次运行目录，由 Computer 建立，Agent 改不了它的位置；它以下的每一级
 * Agent 在沙箱里都能改，可能换成指向沙箱外的符号链接。所以每一级都用 lstat 确认是真正的目录。
 */
async function ensureDirUnder(base: string, path: string): Promise<void> {
  const levels = [base];
  for (const segment of relative(base, path).split("/").filter(Boolean)) {
    levels.push(join(levels[levels.length - 1] ?? base, segment));
  }
  for (const [index, level] of levels.entries()) {
    const info = await lstat(level).catch((error: unknown) => {
      if (isMissing(error)) return undefined;
      throw error;
    });
    if (!info) {
      // base 的上级由 Computer 建立，可以递归创建；base 以下逐级创建。
      await mkdir(level, { recursive: index === 0, mode: 0o700 });
    } else if (info.isSymbolicLink() || !info.isDirectory()) {
      throw new UnsafePathError(`${level} 不是普通目录`);
    }
    await chmod(level, 0o700);
  }
}

/** 先写同目录下的临时文件再改名，读者看到的要么是旧内容，要么是新内容。 */
async function writeFileAtomic(path: string, content: string | Buffer, mode: number): Promise<void> {
  const temporary = join(dirname(path), `.${Date.now()}-${Math.random().toString(16).slice(2)}.tmp`);
  await writeFile(temporary, content, { mode });
  await chmod(temporary, mode);
  await rename(temporary, path);
}

/**
 * 准备本次运行的目录：删除以前运行留下的 `runtime/*`，再建好 `runtime/<id>/bin`。
 * 以前的 RuntimeSession 已经结束，它们的凭证与派生配置都没有用了。
 */
export async function prepareRuntime(root: string, runtimeSessionId: RuntimeSessionId): Promise<RuntimeLayout> {
  const runtimeRoot = join(root, "runtime");
  await ensureDir(root);
  await ensureDir(runtimeRoot);
  for (const entry of await readdir(runtimeRoot)) {
    if (entry !== runtimeSessionId) await rm(join(runtimeRoot, entry), { recursive: true, force: true });
  }
  const runtimeDir = join(runtimeRoot, runtimeSessionId);
  const binDir = join(runtimeDir, "bin");
  await ensureDir(binDir);
  return { root, runtimeDir, binDir };
}

/** 正常退出时删除本次运行的目录。 */
export async function removeRuntime(layout: RuntimeLayout): Promise<void> {
  await rm(layout.runtimeDir, { recursive: true, force: true });
}

/**
 * 生成 `bin/crew`：用 Electron 自带的 Node 运行打包后的 shim。
 * 两个路径都在 `$HOME` 之外，沙箱内可以读取。
 */
export async function writeShimWrapper(
  layout: RuntimeLayout,
  nodeExecutable: string,
  shimEntry: string,
): Promise<string> {
  const path = join(layout.binDir, "crew");
  const script = `#!/bin/sh\nELECTRON_RUN_AS_NODE=1 exec ${shellQuote(nodeExecutable)} ${shellQuote(shimEntry)} "$@"\n`;
  await writeFileAtomic(path, script, 0o700);
  return path;
}

function shellQuote(value: string): string {
  return `'${value.replaceAll("'", `'\\''`)}'`;
}

export function agentLayout(runtime: RuntimeLayout, agentId: AgentId): AgentLayout {
  const home = join(runtime.root, "agents", agentId);
  const runtimeDir = join(runtime.runtimeDir, "agents", agentId);
  const workDir = join(runtime.root, ...agentWorkSegments(agentId));
  return {
    home,
    workDir,
    memoryFile: join(workDir, MEMORY_FILE),
    instructionsFile: join(home, "AGENTS.md"),
    engineDataDir: join(home, "engines", "opencode", "data"),
    sessionFile: join(home, "engines", "opencode", "session.json"),
    runtimeDir,
    tokenFile: join(runtimeDir, "token"),
    configDir: join(runtimeDir, "config"),
    cacheDir: join(runtimeDir, "cache"),
    stateDir: join(runtimeDir, "state"),
  };
}

/**
 * 建好 Agent 的持久目录与本次运行的目录，写入最新的 `AGENTS.md`；还没有 `MEMORY.md` 时写一份模板。可以重复调用。
 */
export async function prepareAgent(runtime: RuntimeLayout, agent: AgentIdentity): Promise<AgentLayout> {
  const layout = agentLayout(runtime, agent.id);
  for (const dir of [layout.workDir, layout.engineDataDir]) await ensureDirUnder(layout.home, dir);
  for (const dir of [layout.configDir, layout.cacheDir, layout.stateDir]) await ensureDirUnder(layout.runtimeDir, dir);
  await writeFileAtomic(layout.instructionsFile, standingInstructions(agent), 0o600);
  await seedMemory(layout, agent);
  return layout;
}

/**
 * 记忆的模板。只在文件不存在时写：已有的内容是 Agent 自己写的，不覆盖。
 * 存在但不是普通文件（例如 Agent 换成了符号链接）时也不动它。
 */
async function seedMemory(layout: AgentLayout, agent: AgentIdentity): Promise<void> {
  const existing = await lstat(layout.memoryFile).catch((error: unknown) => {
    if (isMissing(error)) return undefined;
    throw error;
  });
  if (existing) return;
  await writeFileAtomic(layout.memoryFile, memoryTemplate(agent), 0o600);
}

export function memoryTemplate(agent: Pick<AgentIdentity, "displayName">): string {
  return `# ${agent.displayName}'s memory

What to remember across sessions. Keep this file short: the most important facts here, details in other files next to it.

## About the person

## Ongoing work

## Lessons learned
`;
}

/** 记忆文件的大小（字节）。不存在或不是普通文件时返回 undefined。 */
export async function memorySize(layout: AgentLayout): Promise<number | undefined> {
  const info = await lstat(layout.memoryFile).catch(() => undefined);
  return info?.isFile() ? info.size : undefined;
}

/**
 * 把沙箱外的一个文件复制进 Agent 的缓存目录，例如 OpenCode 的模型价格表。目标已经是同样或更新的副本时跳过。
 * 缓存目录 Agent 写得了，所以逐级确认是真正的目录，写入用先写临时文件再改名，不顺着 Agent 放的符号链接写。
 */
export async function copyIntoCache(layout: AgentLayout, source: string, relativePath: string): Promise<void> {
  const target = join(layout.cacheDir, relativePath);
  const [from, to] = await Promise.all([stat(source), lstat(target).catch(() => undefined)]);
  if (to?.isFile() && to.mtimeMs >= from.mtimeMs) return;
  await ensureDirUnder(layout.runtimeDir, dirname(target));
  await writeFileAtomic(target, await readFile(source), 0o600);
}

export async function writeAgentToken(layout: AgentLayout, token: string): Promise<void> {
  await ensureDirUnder(layout.runtimeDir, layout.runtimeDir);
  await writeFileAtomic(layout.tokenFile, token, 0o600);
}

/** 继续上一段对话的条件：引擎、模型与 `AGENTS.md` 的内容都没变。 */
const SessionRecord = z.object({
  sessionId: z.string().min(1),
  engineId: z.string(),
  model: z.string(),
  instructionsDigest: z.string(),
});
export type SessionRecord = z.infer<typeof SessionRecord>;

export interface SessionKey {
  engineId: string;
  model: string;
  /** 当前 `AGENTS.md` 的内容。 */
  instructions: string;
}

function digest(text: string): string {
  return createHash("sha256").update(text).digest("hex");
}

/** 可以继续的 session ID。没有记录、记录损坏或条件变了时返回 `undefined`，调用方开新 session。 */
export async function resumableSession(layout: AgentLayout, key: SessionKey): Promise<string | undefined> {
  let record: SessionRecord;
  try {
    await ensureDirUnder(layout.home, dirname(layout.sessionFile));
    const info = await lstat(layout.sessionFile).catch((error: unknown) => {
      if (isMissing(error)) return undefined;
      throw error;
    });
    if (!info) return undefined;
    // 命名管道会让读取永远等下去，符号链接会读到别处：只读普通文件。
    if (!info.isFile()) throw new UnsafePathError(`${layout.sessionFile} 不是普通文件`);
    record = SessionRecord.parse(JSON.parse(await readFile(layout.sessionFile, "utf8")));
  } catch (error) {
    // 记录损坏或路径不安全时开新 session：丢掉的只是上下文的连续性，不影响正确性。
    console.error("[computer] session 记录无法读取，开新 session:", error);
    return undefined;
  }
  const matches =
    record.engineId === key.engineId &&
    record.model === key.model &&
    record.instructionsDigest === digest(key.instructions);
  return matches ? record.sessionId : undefined;
}

export async function saveSession(layout: AgentLayout, key: SessionKey, sessionId: string): Promise<void> {
  const record: SessionRecord = {
    sessionId,
    engineId: key.engineId,
    model: key.model,
    instructionsDigest: digest(key.instructions),
  };
  // Turn 期间 Agent 可能改了目录：写之前重新确认，不把文件写到沙箱外。
  await ensureDirUnder(layout.home, dirname(layout.sessionFile));
  await writeFileAtomic(layout.sessionFile, `${JSON.stringify(record, null, 2)}\n`, 0o600);
}

export async function clearSession(layout: AgentLayout): Promise<void> {
  await rm(layout.sessionFile, { force: true });
}

function isInside(parent: string, path: string): boolean {
  const rel = relative(parent, path);
  return rel === "" || (!rel.startsWith("..") && !rel.startsWith("/"));
}

/**
 * Agent 的 Engine 进程树的沙箱范围。
 *
 * 可写：Agent 的持久目录、本次运行的目录与系统临时目录。
 * `$HOME` 之内可读：上面两个目录、`bin/crew` 所在的目录，以及位于 `$HOME` 之内的可执行文件。
 */
export async function confinementFor(
  runtime: RuntimeLayout,
  layout: AgentLayout,
  executables: string[],
): Promise<Confinement> {
  const home = await realpath(homedir());
  const real = (path: string) => realpath(path);
  const agentHome = await real(layout.home);
  const agentRuntime = await real(layout.runtimeDir);
  const binDir = await real(runtime.binDir);
  const executablesInHome = (await Promise.all(executables.map(real))).filter((path) => isInside(home, path));
  return {
    home,
    writable: [agentHome, agentRuntime, await real(tmpdir())],
    homeReadable: [agentHome, agentRuntime, binDir, ...executablesInHome],
  };
}
