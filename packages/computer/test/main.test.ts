import { type ChildProcess, spawn } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";

// 运行真实的 Computer 进程。Server 用一个只回固定状态码的 HTTP 服务代替：
// 这里测的是进程的启动与退出，Server 的行为由 connect.test.ts 覆盖。

const tsx = fileURLToPath(new URL("../../../node_modules/.bin/tsx", import.meta.url));
const entry = fileURLToPath(new URL("../src/main.ts", import.meta.url));

let stub: Server | undefined;
let child: ChildProcess | undefined;

afterEach(async () => {
  child?.kill("SIGKILL");
  child = undefined;
  await new Promise<void>((resolve) => (stub ? stub.close(() => resolve()) : resolve()));
  stub = undefined;
});

async function startStub(status: number): Promise<string> {
  stub = createServer((_request, response) => {
    response.statusCode = status;
    response.end();
  });
  await new Promise<void>((resolve) => stub?.listen(0, "127.0.0.1", resolve));
  const { port } = stub.address() as AddressInfo;
  return `http://127.0.0.1:${port}`;
}

function startComputer(baseUrl: string, crewRoot = join(tmpdir(), `crew-main-test-${process.pid}-${Date.now()}`)) {
  const proc = spawn(tsx, [entry], { stdio: ["pipe", "pipe", "pipe"] });
  child = proc;
  proc.stdin.write(
    `${JSON.stringify({
      runtimeSessionId: "session-1",
      baseUrl,
      computerToken: "token",
      crewRoot,
      shimEntry: "/nonexistent/shim.js",
    })}\n`,
  );
  let stderr = "";
  proc.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  const exited = new Promise<number | null>((resolve) => proc.on("exit", (code) => resolve(code)));
  return { process: proc, exited, stderr: () => stderr };
}

async function firstLine(proc: ChildProcess): Promise<string> {
  if (!proc.stdout) throw new Error("stdout is not piped");
  for await (const line of createInterface({ input: proc.stdout })) {
    return line;
  }
  throw new Error("stdout ended before the first line");
}

describe("computer process", () => {
  it("writes ready and exits when its parent closes stdin", async () => {
    const computer = startComputer(await startStub(204));

    expect(JSON.parse(await firstLine(computer.process))).toEqual({ runtimeSessionId: "session-1" });

    computer.process.stdin?.end();
    expect(await computer.exited).toBe(0);
  }, 15_000);

  it("exits with the reason on stderr when the server rejects its token", async () => {
    const computer = startComputer(await startStub(401));

    expect(await computer.exited).toBe(1);
    expect(computer.stderr()).toContain("Server 拒绝了 Computer 凭证（401）");
  }, 15_000);

  it("exits with the reason instead of idling when it cannot prepare its directories", async () => {
    // crew 目录的位置上是一个普通文件，建不了目录。
    const blocked = join(mkdtempSync(join(tmpdir(), "crew-main-test-")), "crew");
    writeFileSync(blocked, "");
    const computer = startComputer(await startStub(204), blocked);

    expect(await computer.exited).toBe(1);
    expect(computer.stderr()).toContain("启动 Agent 失败");
  }, 15_000);
});
