import { createInterface } from "node:readline";
import { ComputerBootstrap, type ComputerReady, encodeMessage, readMessage } from "@crew/protocol";
import { ServerClient } from "./client";
import { ComputerDaemon } from "./daemon";
import { OpenCodeAdapter } from "./engine/opencode";

// Computer 进程入口，由 Desktop 主进程启动。stdout 只用来写 ready 消息，日志一律写 stderr。

async function main(): Promise<void> {
  const bootstrap = await readMessage(createInterface({ input: process.stdin }), ComputerBootstrap);
  const client = new ServerClient(bootstrap.baseUrl, bootstrap.computerToken);

  await client.connect();

  const ready: ComputerReady = { runtimeSessionId: bootstrap.runtimeSessionId };
  process.stdout.write(encodeMessage(ready));

  // ready 之后再准备 Agent：沙箱自检、读取模型列表与创建 Runner 不应推迟窗口出现。
  const daemon = new ComputerDaemon({
    client,
    engine: new OpenCodeAdapter(),
    runtimeSessionId: bootstrap.runtimeSessionId,
    crewRoot: bootstrap.crewRoot,
    nodeExecutable: process.execPath,
    shimEntry: bootstrap.shimEntry,
  });
  // 准备失败时退出，不空转：主进程看到意外退出会弹出错误对话框。否则界面上的 Agent 一直空闲，没有人回复。
  const started = daemon.start().catch(async (error: unknown) => {
    console.error("[computer] 启动 Agent 失败:", error);
    await daemon.stop().catch((stopError: unknown) => console.error("[computer] 清理失败:", stopError));
    process.exit(1);
  });

  let exiting = false;
  const exit = async () => {
    if (exiting) return;
    exiting = true;
    await started;
    await daemon.stop();
    process.exit(0);
  };
  process.once("SIGTERM", exit);
  process.once("SIGINT", exit);
  // 父进程退出（包括被 SIGKILL）时 stdin 关闭，Computer 随之退出，不留下孤儿进程。
  // 保持 stdin 读取也让进程在没有其他工作时继续运行。
  process.stdin.once("end", exit);
  process.stdin.resume();
}

main().catch((error: unknown) => {
  console.error("[computer] 启动失败:", error);
  process.exit(1);
});
