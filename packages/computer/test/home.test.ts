import { execFileSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, readdirSync, statSync, symlinkSync } from "node:fs";
import { mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { AgentId, RuntimeSessionId } from "@crew/protocol";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  clearSession,
  confinementFor,
  prepareAgent,
  prepareRuntime,
  removeRuntime,
  resumableSession,
  saveSession,
  writeAgentToken,
  writeShimWrapper,
} from "../src/home";
import { standingInstructions } from "../src/instructions";

// 目录布局在临时根目录下测试，不碰真实的 ~/.crew。

const alice = {
  id: AgentId.parse("2f8c0b6e-3a1d-4c5e-9f7a-1b2c3d4e5f60"),
  displayName: "Alice",
  persona: "你是一位严谨的代码审查者。",
};
const session = RuntimeSessionId.parse("session-1");

let root: string;
beforeEach(async () => {
  root = await realpath(await mkdtemp(join(tmpdir(), "crew-home-test-")));
});
afterEach(async () => {
  await rm(root, { recursive: true, force: true });
});

const mode = (path: string) => statSync(path).mode & 0o777;

describe("standingInstructions", () => {
  it("matches the reviewed text", async () => {
    await expect(standingInstructions(alice)).toMatchFileSnapshot("./__snapshots__/AGENTS.md");
  });

  it("contains no path or time, so it only changes with the agent's settings", () => {
    const text = standingInstructions(alice);
    expect(text).not.toContain(homedir());
    expect(text).not.toMatch(/\d{4}-\d{2}-\d{2}/);
  });
});

describe("prepareRuntime", () => {
  it("removes runtime directories left by earlier runs and keeps the current one", async () => {
    mkdirSync(join(root, "runtime", "old-session", "bin"), { recursive: true });
    const layout = await prepareRuntime(root, session);
    expect(existsSync(join(root, "runtime", "old-session"))).toBe(false);
    expect(existsSync(layout.binDir)).toBe(true);
    expect(mode(layout.runtimeDir)).toBe(0o700);
  });

  it("is removed by removeRuntime", async () => {
    const layout = await prepareRuntime(root, session);
    await removeRuntime(layout);
    expect(existsSync(layout.runtimeDir)).toBe(false);
  });
});

describe("writeShimWrapper", () => {
  it("runs the shim with Electron as Node and quotes both paths", async () => {
    const layout = await prepareRuntime(root, session);
    const path = await writeShimWrapper(layout, "/Applications/Crew App.app/Electron", "/opt/it's/shim.js");
    expect(await readFile(path, "utf8")).toBe(
      "#!/bin/sh\nELECTRON_RUN_AS_NODE=1 exec '/Applications/Crew App.app/Electron' '/opt/it'\\''s/shim.js' \"$@\"\n",
    );
    expect(mode(path)).toBe(0o700);
  });
});

describe("prepareAgent", () => {
  it("creates the directories and writes AGENTS.md with private permissions", async () => {
    const runtime = await prepareRuntime(root, session);
    const layout = await prepareAgent(runtime, alice);
    for (const dir of [layout.workDir, layout.engineDataDir, layout.configDir, layout.cacheDir, layout.stateDir]) {
      expect(mode(dir)).toBe(0o700);
    }
    expect(await readFile(layout.instructionsFile, "utf8")).toBe(standingInstructions(alice));
    expect(mode(layout.instructionsFile)).toBe(0o600);
  });

  it("can run again and rewrites AGENTS.md when the persona changes", async () => {
    const runtime = await prepareRuntime(root, session);
    await prepareAgent(runtime, alice);
    const changed = { ...alice, persona: "你是一位测试工程师。" };
    const layout = await prepareAgent(runtime, changed);
    expect(await readFile(layout.instructionsFile, "utf8")).toContain("测试工程师");
  });

  it("writes the token readable only by its owner", async () => {
    const runtime = await prepareRuntime(root, session);
    const layout = await prepareAgent(runtime, alice);
    await writeAgentToken(layout, "token-123");
    expect(await readFile(layout.tokenFile, "utf8")).toBe("token-123");
    expect(mode(layout.tokenFile)).toBe(0o600);
  });
});

describe("session continuity", () => {
  const key = { engineId: "opencode", model: "opencode-go/deepseek-v4-pro", instructions: standingInstructions(alice) };

  it("resumes the saved session while engine, model and instructions are unchanged", async () => {
    const layout = await prepareAgent(await prepareRuntime(root, session), alice);
    expect(await resumableSession(layout, key)).toBeUndefined();
    await saveSession(layout, key, "ses_abc");
    expect(await resumableSession(layout, key)).toBe("ses_abc");
  });

  it("starts over when the model or the instructions change", async () => {
    const layout = await prepareAgent(await prepareRuntime(root, session), alice);
    await saveSession(layout, key, "ses_abc");
    expect(await resumableSession(layout, { ...key, model: "deepseek/deepseek-v4-pro" })).toBeUndefined();
    expect(await resumableSession(layout, { ...key, instructions: `${key.instructions}\nmore` })).toBeUndefined();
  });

  it("starts over when the record is corrupt or cleared", async () => {
    const layout = await prepareAgent(await prepareRuntime(root, session), alice);
    await writeFile(layout.sessionFile, "{ not json");
    expect(await resumableSession(layout, key)).toBeUndefined();
    await saveSession(layout, key, "ses_abc");
    await clearSession(layout);
    expect(await resumableSession(layout, key)).toBeUndefined();
  });
});

// Agent 在沙箱里能写自己的目录，可能把其中的目录换成指向别处的符号链接，或把文件换成命名管道。
// Computer 不受沙箱约束，不能顺着这些路径操作。
describe("paths the agent controls", () => {
  const key = { engineId: "opencode", model: "opencode-go/deepseek-v4-pro", instructions: standingInstructions(alice) };

  /** 沙箱外的一个目录，权限 0755。 */
  function outsideDir(): string {
    const dir = join(root, "outside");
    mkdirSync(dir);
    chmodSync(dir, 0o755);
    return dir;
  }

  it("refuses to prepare an agent whose work directory was replaced by a link", async () => {
    const runtime = await prepareRuntime(root, session);
    const layout = await prepareAgent(runtime, alice);
    const outside = outsideDir();
    await rm(layout.workDir, { recursive: true });
    symlinkSync(outside, layout.workDir);

    await expect(prepareAgent(runtime, alice)).rejects.toThrow("不是普通目录");
    expect(mode(outside)).toBe(0o755);
  });

  it("refuses to save a session through a directory replaced by a link", async () => {
    const layout = await prepareAgent(await prepareRuntime(root, session), alice);
    const outside = outsideDir();
    const engineDir = join(layout.home, "engines", "opencode");
    await rm(engineDir, { recursive: true });
    symlinkSync(outside, engineDir);

    await expect(saveSession(layout, key, "ses_abc")).rejects.toThrow("不是普通目录");
    expect(readdirSync(outside)).toEqual([]);
  });

  it("ignores a session record that is not a regular file instead of blocking on it", async () => {
    const layout = await prepareAgent(await prepareRuntime(root, session), alice);
    execFileSync("/usr/bin/mkfifo", [layout.sessionFile]);
    const started = Date.now();
    expect(await resumableSession(layout, key)).toBeUndefined();
    expect(Date.now() - started).toBeLessThan(1_000);
  });
});

describe("confinementFor", () => {
  it("lets the agent write its own directories and the temp directory, and read the shim directory", async () => {
    const runtime = await prepareRuntime(root, session);
    const layout = await prepareAgent(runtime, alice);
    const confinement = await confinementFor(runtime, layout, ["/usr/bin/true"]);
    expect(confinement.home).toBe(await realpath(homedir()));
    expect(confinement.writable).toEqual([
      await realpath(layout.home),
      await realpath(layout.runtimeDir),
      await realpath(tmpdir()),
    ]);
    expect(confinement.homeReadable).toEqual([
      await realpath(layout.home),
      await realpath(layout.runtimeDir),
      await realpath(runtime.binDir),
    ]);
  });
});
