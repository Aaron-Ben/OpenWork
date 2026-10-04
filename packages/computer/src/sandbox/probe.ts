import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { classifySandboxExit } from "./outcome";
import { SANDBOX_EXEC } from "./profile";

export type SandboxStatus = { available: true } | { available: false; reason: string };

/** 自检用的 profile：拒绝一切写入，只放行 `/dev/null`。 */
const PROBE_PROFILE = '(version 1)\n(allow default)\n(deny file-write*)\n(allow file-write* (literal "/dev/null"))\n';

interface RunResult {
  exitCode: number;
  stderr: string;
}

function run(args: string[]): Promise<RunResult> {
  return new Promise((resolve, reject) => {
    execFile(SANDBOX_EXEC, args, (error, _stdout, stderr) => {
      if (error && typeof error.code !== "number") {
        reject(error);
        return;
      }
      resolve({ exitCode: typeof error?.code === "number" ? error.code : 0, stderr });
    });
  });
}

/**
 * Computer 启动时的沙箱自检。先确认 `sandbox-exec` 能运行命令，再确认一次被禁止的写入确实被内核拒绝。
 * 自检不通过时不启动任何 Agent，没有无沙箱的运行路径。
 */
export async function probeSandbox(): Promise<SandboxStatus> {
  if (process.platform !== "darwin") {
    return { available: false, reason: "沙箱只支持 macOS" };
  }
  const directory = await realpath(await mkdtemp(join(tmpdir(), "crew-sandbox-probe-")));
  try {
    const shell = (script: string, ...args: string[]) =>
      run(["-p", PROBE_PROFILE, "--", "/bin/sh", "-c", script, "sh", ...args]);

    let control: RunResult;
    try {
      control = await shell("exit 0");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      return { available: false, reason: `无法启动 ${SANDBOX_EXEC}：${message}` };
    }
    if (control.exitCode !== 0) {
      return { available: false, reason: `${SANDBOX_EXEC} 无法运行命令：${control.stderr.split("\n")[0] ?? ""}` };
    }

    const target = join(directory, "probe");
    const denied = await shell('printf probe > "$1"', target);
    if (existsSync(target)) {
      return { available: false, reason: `沙箱禁止的写入成功了（${target}），拒绝运行 Agent` };
    }
    const outcome = classifySandboxExit(denied.exitCode, denied.stderr);
    switch (outcome.kind) {
      case "denied":
        return { available: true };
      case "sandbox-failed":
        return { available: false, reason: outcome.message };
      case "completed":
        return {
          available: false,
          reason: `被禁止的写入失败了，但没有出现预期的 EPERM（退出码 ${denied.exitCode}）`,
        };
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}
