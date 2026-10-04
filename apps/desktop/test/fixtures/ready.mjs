// 读入一行 bootstrap，原样回写其中的 runtimeSessionId 作为 ready，然后一直运行。
import { createInterface } from "node:readline";

for await (const line of createInterface({ input: process.stdin })) {
  const { runtimeSessionId } = JSON.parse(line);
  process.stdout.write(`${JSON.stringify({ runtimeSessionId })}\n`);
  break;
}
setInterval(() => {}, 1_000);
