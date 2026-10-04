// 测试用的假 opencode：按环境变量 FAKE_MODE 模拟成功、报错、session 不存在、卡住与刷屏。
// 由 Electron 以 Node 方式运行（真实的 Node 装在 $HOME 下，沙箱读不到）。
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const args = process.argv.slice(2);
const sessionArg = args.includes("--session") ? args[args.indexOf("--session") + 1] : undefined;
const mode = process.env.FAKE_MODE ?? "ok";
const emit = (event) => process.stdout.write(`${JSON.stringify(event)}\n`);

let prompt = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => (prompt += chunk));
process.stdin.on("end", () => {
  const sessionID = sessionArg ?? "ses_fake_new";
  if (mode === "ok" || (mode === "session-missing" && !sessionArg)) {
    writeFileSync(
      "received.json",
      JSON.stringify({
        args,
        prompt,
        cwd: process.cwd(),
        env: Object.fromEntries(
          [
            "HOME",
            "XDG_DATA_HOME",
            "XDG_CONFIG_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
            "OPENCODE_DISABLE_PROJECT_CONFIG",
            "OPENCODE_AUTH_CONTENT",
            "OPENCODE_CONFIG_CONTENT",
            "CREW_SERVER_URL",
            "DATABASE_URL",
          ].map((k) => [k, process.env[k] ?? null]),
        ),
      }),
    );
    emit({ type: "step_start", sessionID });
    emit({ type: "text", sessionID, part: { text: "done" } });
    emit({ type: "step_finish", sessionID });
    process.exit(0);
  }
  if (mode === "session-missing") {
    process.stderr.write("\u001b[91m\u001b[1mError: \u001b[0mSession not found\n");
    process.exit(1);
  }
  if (mode === "model-missing") {
    process.stderr.write(
      'level=ERROR message=failed error="ProviderModelNotFoundError: Model not found: nosuch/model."\n',
    );
    emit({ type: "error", sessionID, error: { name: "UnknownError", data: { message: "Unexpected server error." } } });
    process.exit(1);
  }
  if (mode === "hang") {
    process.on("SIGINT", () => {}); // 忽略 SIGINT，迫使调用方升级为 SIGKILL
    const grandchild = spawn("/bin/sleep", ["30"], { stdio: "ignore" });
    writeFileSync("grandchild.pid", String(grandchild.pid));
    setInterval(() => {}, 1_000);
    return;
  }
  if (mode === "background" || mode === "background-holding-stdout") {
    // Agent 用 `cmd &` 起了一个后台进程，然后这一轮正常结束。
    // holding-stdout 时后台进程继承了 opencode 的 stdout，管道因此一直不关。
    const stdio = mode === "background" ? "ignore" : ["ignore", "inherit", "ignore"];
    const background = spawn("/bin/sleep", ["30"], { stdio });
    writeFileSync("background.pid", String(background.pid));
    emit({ type: "step_start", sessionID });
    emit({ type: "step_finish", sessionID });
    process.exit(0);
  }
  if (mode === "flood") {
    const line = `${JSON.stringify({ type: "text", sessionID, part: { text: "x".repeat(1000) } })}\n`;
    const write = () => {
      while (process.stdout.write(line)) {}
      process.stdout.once("drain", write);
    };
    write();
  }
});
