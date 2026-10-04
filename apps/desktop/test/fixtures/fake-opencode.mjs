// 冒烟测试用的假 opencode：不调用模型，像真实 Agent 一样经 `crew reply` 回复。
// 由 Electron 以 Node 方式运行，在 Computer 启动的 Seatbelt 里执行。
import { spawn } from "node:child_process";

const args = process.argv.slice(2);

// Computer 启动时用 `opencode models` 读取可用模型。
if (args[0] === "models") {
  process.stdout.write("fake/model\n");
  process.exit(0);
}

const sessionArg = args.includes("--session") ? args[args.indexOf("--session") + 1] : undefined;
const sessionID = sessionArg ?? "ses_smoke";
const emit = (event) => process.stdout.write(`${JSON.stringify(event)}\n`);

let prompt = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => (prompt += chunk));
process.stdin.on("end", () => {
  // 每轮输入按房间列出未读消息，房间标题是 `# Room <room-id> (direct)`。
  const roomId = prompt.match(/^# Room ([0-9a-f-]{36}) /m)?.[1];
  if (!roomId) {
    process.stderr.write("prompt 里没有房间\n");
    process.exit(1);
  }
  // 回复里带上收到的消息与 shell 会展开的字符，证明输入送到了，正文也原样到达 Server。
  const body = prompt.includes("ping") ? "pong：`ls $HOME` 原样保留" : "没有收到 ping";
  const crew = spawn("crew", ["reply", roomId], { stdio: ["pipe", "ignore", "inherit"] });
  crew.stdin.end(`${body}\n`);
  crew.on("exit", (code) => {
    emit({ type: "step_start", sessionID });
    emit({ type: "step_finish", sessionID });
    process.exit(code ?? 1);
  });
});
