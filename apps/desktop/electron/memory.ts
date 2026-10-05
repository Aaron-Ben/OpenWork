import { constants } from "node:fs";
import { lstat, open, realpath } from "node:fs/promises";
import { join } from "node:path";
import { AgentId, agentWorkSegments, MEMORY_FILE } from "@crew/protocol";
import type { AgentMemory } from "./contract";

/** 界面最多读这么多字节：记忆本该很短，超出的部分不显示，界面写明被截短了。 */
export const MEMORY_READ_MAX = 256 * 1024;

/**
 * 读一个 Agent 的 `MEMORY.md` 给界面看。Agent ID 不合法、文件不存在或不是普通文件时返回 null。
 *
 * Agent 能改自己的整个目录：可能把 `work` 换成指向别处的链接，把 `MEMORY.md` 换成符号链接或命名管道，
 * 还可能在检查与打开之间来回切换。所以：先确认 `work` 是 Agent 目录下真正的目录；打开时不跟随最后一级链接、
 * 不阻塞（命名管道不会卡住主进程）；打开后对句柄确认是普通文件再读。
 */
export async function readAgentMemory(crewRoot: string, agentId: unknown): Promise<AgentMemory | null> {
  const id = AgentId.safeParse(agentId);
  if (!id.success) return null;
  const workDir = join(crewRoot, ...agentWorkSegments(id.data));
  const home = join(workDir, "..");
  const [homeReal, workReal, workInfo] = await Promise.all([
    realpath(home).catch(() => undefined),
    realpath(workDir).catch(() => undefined),
    lstat(workDir).catch(() => undefined),
  ]);
  if (!homeReal || !workInfo?.isDirectory() || workReal !== join(homeReal, "work")) return null;

  const file = await open(
    join(workDir, MEMORY_FILE),
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK,
  ).catch(() => undefined);
  if (!file) return null;
  try {
    const info = await file.stat();
    if (!info.isFile()) return null;
    const buffer = Buffer.alloc(Math.min(info.size, MEMORY_READ_MAX));
    const { bytesRead } = await file.read(buffer, 0, buffer.length, 0);
    return {
      content: buffer.subarray(0, bytesRead).toString("utf8"),
      bytes: info.size,
      truncated: info.size > MEMORY_READ_MAX,
      modifiedAt: info.mtime.toISOString(),
    };
  } finally {
    await file.close();
  }
}
