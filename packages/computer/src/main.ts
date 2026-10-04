import { createInterface } from "node:readline";
import { ComputerBootstrap, type ComputerReady, encodeMessage, readMessage } from "@crew/protocol";
import { connectToServer } from "./connect";

// Computer 进程入口，由 Desktop 主进程启动。stdout 只用来写 ready 消息，日志一律写 stderr。

async function main(): Promise<void> {
  const bootstrap = await readMessage(createInterface({ input: process.stdin }), ComputerBootstrap);

  await connectToServer(bootstrap.baseUrl, bootstrap.computerToken);

  const ready: ComputerReady = { runtimeSessionId: bootstrap.runtimeSessionId };
  process.stdout.write(encodeMessage(ready));

  const exit = () => process.exit(0);
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
