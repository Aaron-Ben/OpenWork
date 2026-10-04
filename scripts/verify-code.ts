import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { checkCode } from "./code-rules";

// 代码约定检查：packages/AGENTS.md 中 Biome 管不到、但能机械检查的规则。由 `pnpm lint` 运行，有问题时以 1 退出。

const root = resolve(import.meta.dirname, "..");
/** 检查器自己的源码与测试在注释和字符串里写着被禁止的写法，不检查。 */
const SELF = new Set(["scripts/code-rules.ts", "scripts/code-rules.test.ts"]);

/** 新版 Crew 的源码与测试。旧版 Rust 加 Tauri 的 `desktop/` 不检查。 */
function sourceFiles(): string[] {
  const output = execFileSync(
    "git",
    ["ls-files", "--cached", "--others", "--exclude-standard", "--", "packages", "apps", "scripts"],
    { cwd: root, encoding: "utf8" },
  );
  return output
    .split("\n")
    .filter((file) => /\.(?:ts|tsx|mjs|cjs|js)$/.test(file) && !SELF.has(file))
    .filter((file) => existsSync(resolve(root, file)));
}

const problems: string[] = [];
for (const file of sourceFiles()) {
  for (const { line, message } of checkCode(readFileSync(resolve(root, file), "utf8"))) {
    problems.push(`${file}:${line}：${message}`);
  }
}

if (problems.length > 0) {
  console.error(problems.join("\n"));
  console.error(`\n代码约定检查发现 ${problems.length} 个问题。`);
  process.exit(1);
}
console.log("代码约定检查通过。");
