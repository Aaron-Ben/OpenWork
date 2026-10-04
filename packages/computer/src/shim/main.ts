import { readFile } from "node:fs/promises";
import { text } from "node:stream/consumers";
import { runCli } from "./cli";

// `crew` 命令的入口。Computer 在运行期目录生成的 `bin/crew` 以 ELECTRON_RUN_AS_NODE 运行它。

runCli(process.argv.slice(2), {
  env: process.env,
  // stdin 是终端时没有人会写入，直接当作空，避免一直等待。
  readStdin: () => (process.stdin.isTTY ? Promise.resolve("") : text(process.stdin)),
  readFile: (path) => readFile(path, "utf8"),
  stdout: (output) => process.stdout.write(output),
  stderr: (output) => process.stderr.write(output),
  fetch,
}).then((code) => {
  // 设置退出码而不是调用 process.exit，让 stdout 与 stderr 写完。
  process.exitCode = code;
});
