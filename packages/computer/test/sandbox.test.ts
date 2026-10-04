import { execFile } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { mkdtemp, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import {
  buildProfile,
  type Confinement,
  classifySandboxExit,
  probeSandbox,
  SANDBOX_EXEC,
  sandboxArgs,
} from "../src/sandbox";

// 在真实的 Seatbelt 下运行命令。用临时目录里的假主目录代替真实的 $HOME，测试不读写用户自己的文件。

let root: string;
let home: string;
let agentDir: string;
let workDir: string;
let trickyDir: string;
let confinement: Confinement;

beforeAll(async () => {
  root = await realpath(await mkdtemp(join(tmpdir(), "crew-sandbox-test-")));
  home = join(root, "home");
  agentDir = join(home, "agent");
  workDir = join(root, "work");
  // 名字里带着 profile 语法的目录：路径只经参数传入，不能改变规则。
  trickyDir = join(home, 'x")(allow default');
  for (const dir of [home, agentDir, workDir, trickyDir, join(root, "outside")]) mkdirSync(dir, { recursive: true });
  writeFileSync(join(home, "secret.txt"), "secret");
  writeFileSync(join(agentDir, "notes.txt"), "notes");
  writeFileSync(join(root, "outside", "public.txt"), "public");
  confinement = { home, writable: [agentDir, workDir, trickyDir], homeReadable: [agentDir, trickyDir] };
});

afterAll(async () => {
  await rm(root, { recursive: true, force: true });
});

interface Result {
  exitCode: number;
  stdout: string;
  stderr: string;
}

function sandboxed(script: string): Promise<Result> {
  const args = sandboxArgs(buildProfile(confinement), ["/bin/sh", "-c", script]);
  return new Promise((resolve) => {
    execFile(SANDBOX_EXEC, args, (error, stdout, stderr) => {
      resolve({ exitCode: typeof error?.code === "number" ? error.code : 0, stdout, stderr });
    });
  });
}

describe("Seatbelt profile", () => {
  it("never puts paths into the profile text", () => {
    const { text, parameters } = buildProfile(confinement);
    expect(text).not.toContain(root);
    expect(text).not.toContain("allow default)\n(allow");
    expect(parameters.map(([, value]) => value)).toContain(trickyDir);
  });

  it("allows reading files outside the home directory", async () => {
    const result = await sandboxed(`cat "${join(root, "outside", "public.txt")}"`);
    expect(result).toMatchObject({ exitCode: 0, stdout: "public" });
  });

  it("denies reading other files in the home directory", async () => {
    const result = await sandboxed(`cat "${join(home, "secret.txt")}"`);
    expect(result.exitCode).not.toBe(0);
    expect(classifySandboxExit(result.exitCode, result.stderr)).toEqual({ kind: "denied" });
  });

  it("allows reading the agent's own directory", async () => {
    const result = await sandboxed(`cat "${join(agentDir, "notes.txt")}"`);
    expect(result).toMatchObject({ exitCode: 0, stdout: "notes" });
  });

  it("still allows checking that a home file exists", async () => {
    const result = await sandboxed(`test -e "${join(home, "secret.txt")}"`);
    expect(result.exitCode).toBe(0);
  });

  it("allows writing only to the writable directories", async () => {
    expect((await sandboxed(`echo a > "${join(agentDir, "out.txt")}"`)).exitCode).toBe(0);
    expect((await sandboxed(`echo a > "${join(workDir, "out.txt")}"`)).exitCode).toBe(0);
    expect((await sandboxed(`echo a > "${join(home, "out.txt")}"`)).exitCode).not.toBe(0);
    expect((await sandboxed(`echo a > "${join(root, "outside", "out.txt")}"`)).exitCode).not.toBe(0);
  });

  it("treats a directory named like profile syntax as a plain path", async () => {
    expect((await sandboxed(`echo a > '${join(trickyDir, "out.txt")}'`)).exitCode).toBe(0);
    expect((await sandboxed(`echo a > "${join(home, "other.txt")}"`)).exitCode).not.toBe(0);
  });

  it("allows writing to /dev/null", async () => {
    expect((await sandboxed("echo a > /dev/null")).exitCode).toBe(0);
  });
});

describe("classifySandboxExit", () => {
  it("recognizes sandbox-exec's own failures", () => {
    expect(classifySandboxExit(65, "sandbox-exec: unbound variable: bogus at <input string>, line 1\n")).toEqual({
      kind: "sandbox-failed",
      message: "sandbox-exec: unbound variable: bogus at <input string>, line 1",
    });
  });

  it("keeps a command's own exit code 65 as a completed run", () => {
    expect(classifySandboxExit(65, "my tool: bad input\n")).toEqual({ kind: "completed" });
  });

  it("reports a real profile error as a sandbox failure", async () => {
    const result = await new Promise<Result>((resolve) => {
      execFile(SANDBOX_EXEC, ["-p", "(version 1)(allow bogus)", "--", "/usr/bin/true"], (error, stdout, stderr) => {
        resolve({ exitCode: typeof error?.code === "number" ? error.code : 0, stdout, stderr });
      });
    });
    expect(classifySandboxExit(result.exitCode, result.stderr).kind).toBe("sandbox-failed");
  });
});

describe("probeSandbox", () => {
  it("reports the sandbox as available on this machine", async () => {
    expect(await probeSandbox()).toEqual({ available: true });
  });
});
