import { fileURLToPath } from "node:url";
import { ComputerReady } from "@crew/protocol";
import { afterEach, describe, expect, it } from "vitest";
import { type Child, ChildStartError, startChild } from "../src/main/child";

// 用 node 运行 test/fixtures 下的小脚本代替 Server 与 Computer，测试启动、失败与停止的监管逻辑。

const fixture = (name: string) => fileURLToPath(new URL(`./fixtures/${name}`, import.meta.url));

const started: Child<unknown>[] = [];
afterEach(async () => {
  await Promise.all(started.splice(0).map((child) => child.stop()));
});

function start(entry: string, extra: { readyTimeoutMs?: number; stopGraceMs?: number } = {}) {
  return startChild({
    name: "Fixture",
    executable: process.execPath,
    entry: fixture(entry),
    env: process.env,
    bootstrap: { runtimeSessionId: "s-1" },
    readySchema: ComputerReady,
    ...extra,
  }).then((child) => {
    started.push(child);
    return child;
  });
}

describe("startChild", () => {
  it("returns the ready message written by the child", async () => {
    const child = await start("ready.mjs");
    expect(child.ready).toEqual({ runtimeSessionId: "s-1" });
  });

  it("fails with the stderr tail when the child exits before ready", async () => {
    const error = await start("crash.mjs").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ChildStartError);
    expect((error as ChildStartError).message).toContain("在 ready 之前退出");
    expect((error as ChildStartError).stderr).toContain("数据库连接失败");
  });

  it("fails when the child does not report ready in time", async () => {
    const error = await start("hang.mjs", { readyTimeoutMs: 300 }).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ChildStartError);
    expect((error as ChildStartError).message).toContain("秒内没有报告 ready");
  });

  it("reports an exit that stop() did not cause", async () => {
    const child = await start("ready.mjs");
    const exited = new Promise<void>((resolve) => child.onUnexpectedExit(() => resolve()));
    process.kill(child.pid, "SIGKILL");
    await exited;
  });

  it("does not report the exit caused by stop()", async () => {
    const child = await start("ready.mjs");
    let reported = false;
    child.onUnexpectedExit(() => {
      reported = true;
    });
    await child.stop();
    expect(reported).toBe(false);
  });

  it("kills a child that ignores SIGTERM after the grace period", async () => {
    const child = await start("stubborn.mjs", { stopGraceMs: 300 });
    const before = Date.now();
    await child.stop();
    expect(Date.now() - before).toBeGreaterThanOrEqual(250);
  });
});
