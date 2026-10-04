# 代码约定

本文是 `packages/` 与 `apps/` 下 TypeScript 代码的约定，补充根 [AGENTS.md](../AGENTS.md)。`packages/CLAUDE.md` 与 `apps/CLAUDE.md` 是指向本文的软链接。理由见 [设计 Agent Note](../.agents/notes/proposed/architecture/2026-10-04-typescript-rewrite.md)。

## 类型与校验

- 跨进程或跨包传递的 ID 用 branded 类型（zod 的 `.brand()`），不用裸 `string`。例子是 `packages/protocol/src/runtime.ts` 的 `RuntimeSessionId`。
- 同一进程内信任 TypeScript 类型。只在 HTTP、stdin 与 stdout、文件、子进程输出、环境变量与配置处校验输入。
- 可辨识联合用 `switch` 处理，封闭的联合以 `assertNever` 结尾。
- 禁止 `as unknown`。

## 行为

- 只在操作成功后发通知、更新派生状态。
- 配置缺失或错误时，在启动时报错退出，不跳过。
- 空的 `catch` 写明吞掉的错误与原因。
- Server 与 Computer 的 stdout 只用来写协议消息，日志一律写 stderr。

## 结构

- 包之间直接引用源码：`package.json` 的 `exports` 指向 `src/index.ts`，没有构建步骤。
- 测试放在包的 `test/` 目录，不放进 `src/`。
