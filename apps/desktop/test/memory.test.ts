import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { MEMORY_READ_MAX, readAgentMemory } from "../electron/memory";

// 主进程读 Agent 的 MEMORY.md 给界面看：只读 Agent 工作目录里的普通文件。

const agentId = "2f8c0b6e-3a1d-4c5e-9f7a-1b2c3d4e5f60";
let root: string;
let workDir: string;

beforeEach(async () => {
  root = await mkdtemp(join(tmpdir(), "crew-memory-"));
  workDir = join(root, "agents", agentId, "work");
  await mkdir(workDir, { recursive: true });
});

afterEach(async () => {
  await rm(root, { recursive: true, force: true });
});

describe("readAgentMemory", () => {
  it("reads the agent's MEMORY.md with its size and time", async () => {
    await writeFile(join(workDir, "MEMORY.md"), "# 记忆\n用户喜欢简短的回复。\n");
    const memory = await readAgentMemory(root, agentId);
    expect(memory).toMatchObject({ content: "# 记忆\n用户喜欢简短的回复。\n", truncated: false });
    expect(memory?.bytes).toBe(Buffer.byteLength("# 记忆\n用户喜欢简短的回复。\n"));
  });

  it("reads only the beginning of a file over the limit and says so", async () => {
    await writeFile(join(workDir, "MEMORY.md"), "a".repeat(MEMORY_READ_MAX + 10));
    const memory = await readAgentMemory(root, agentId);
    expect(memory?.content).toHaveLength(MEMORY_READ_MAX);
    expect(memory).toMatchObject({ bytes: MEMORY_READ_MAX + 10, truncated: true });
  });

  it("returns null for a missing file, a link the agent put there, and an id that is not an agent id", async () => {
    expect(await readAgentMemory(root, agentId)).toBeNull();
    const elsewhere = join(root, "secret.txt");
    await writeFile(elsewhere, "不是记忆");
    await symlink(elsewhere, join(workDir, "MEMORY.md"));
    expect(await readAgentMemory(root, agentId)).toBeNull();
    expect(await readAgentMemory(root, "../../etc")).toBeNull();
  });

  it("does not follow a work directory the agent replaced with a link, nor block on a named pipe", async () => {
    const other = join(root, "other");
    await mkdir(other);
    await writeFile(join(other, "MEMORY.md"), "别的 Agent 的记忆");
    await rm(workDir, { recursive: true });
    await symlink(other, workDir);
    expect(await readAgentMemory(root, agentId)).toBeNull();

    await rm(workDir);
    await mkdir(workDir);
    execFileSync("/usr/bin/mkfifo", [join(workDir, "MEMORY.md")]);
    expect(await readAgentMemory(root, agentId)).toBeNull();
  });
});
