# 代码约定

本文是 `packages/` 与 `apps/` 下 TypeScript 代码的约定，补充根 [AGENTS.md](../AGENTS.md)。`packages/CLAUDE.md` 与 `apps/CLAUDE.md` 是指向本文的软链接。每条规则后半句是理由，这些约定的来历见 [Agent Note：仓库规则、检查与测试流程](../.agents/notes/implemented/process/2026-10-04-repo-rules-and-checks.md)。

## 类型与校验

- 跨进程或跨包传递的 ID 用 branded 类型（zod 的 `.brand()`），不用裸 `string`：把 `RoomId` 传给需要 `AgentId` 的地方时，编译器会报错。例子见 `packages/protocol/src/ids.ts`。
- 同一进程内信任 TypeScript 类型。只在 HTTP、stdin 与 stdout、文件、子进程输出、环境变量与配置处校验输入：这些数据来自别的进程或用户，类型系统管不到。
- 可辨识联合用 `switch` 处理，封闭的联合以 `assertNever` 结尾：新增一种情况时，漏处理的地方会在类型检查中报错。`assertNever` 从 `@crew/protocol` 导入，不各写一份。
- 禁止 `as unknown`：它让任何类型互相转换，等于关掉类型检查。`pnpm lint` 检查这一条。

## 行为

- 只在操作成功后发通知、更新派生状态：否则操作失败时，通知与派生状态会和真实数据不一致。
- 配置缺失或错误时，在启动时报错退出，不跳过：跳过的配置会在运行中以别的形式出错，更难排查。
- 空的 `catch` 写明吞掉的错误与原因。`pnpm lint` 检查这一条。
- Server 与 Computer 的 stdout 只用来写协议消息，日志一律写 stderr：主进程把 stdout 的第一行当作 ready 消息解析，混进一行日志就会启动失败。Biome 禁止这两个包使用 `console.log`。

## 结构

- 包之间直接引用源码：`package.json` 的 `exports` 指向 `src/index.ts`，没有构建步骤。
- 测试放在包的 `test/` 目录，不放进 `src/`。
- 每个包有一份 `README.md`，按[包 README 模板](../docs/templates/package-readme.md)写。包的入口、文件、模型可见的内容或限制改变时，在同一个改动里更新它。
