# Agent Note: 仓库规则、检查与测试流程

Status: implemented

## 问题

这是单人项目，代码主要由 AI 写。规则要满足几件事：Claude Code 与 Codex 读到同一份规则；能机械检查的规则真正被执行，而不只写在文档里；讨论出的决策与理由不在长对话中丢失。DSH 有完整的文档与检查体系可以参考，但它服务于多人协作的大型仓库。

## 决策

**规则文件**

- 根目录的规则文件是 `AGENTS.md`，`CLAUDE.md` 是指向它的软链接，Claude Code 与 Codex 读到同一份规则。DSH 的根 `dsh:CLAUDE.md` 就是指向 `AGENTS.md` 的软链接。
- 根 `AGENTS.md` 只放每个会话都需要的常驻规则，有字数上限（`scripts/agents-budget.ts`）。
- `packages/AGENTS.md` 只放 `packages/` 与 `apps/` 下的代码约定，`packages/CLAUDE.md` 与 `apps/CLAUDE.md` 是指向它的软链接。做法来自 DSH 的 `dsh:packages/AGENTS.md`；代码约定的条目来自 DSH 的根 `dsh:AGENTS.md` 与 `dsh:packages/AGENTS.md`。
- `docs/AGENTS.md` 只放文档标准，做法来自 DSH 的 `dsh:docs/AGENTS.md`。`docs/CLAUDE.md` 是指向它的软链接。
- 子目录都用 `CLAUDE.md` 软链接，是因为根目录有 `CLAUDE.md` 时，Claude Code 不读任何 `AGENTS.md`；子目录的 `CLAUDE.md` 在读到这个目录里的文件时才加载（Claude Code 官方文档的 memory 页）。文档标准还适用于 `docs/` 以外的 Markdown，所以根 `AGENTS.md` 另外要求写 Markdown 前先读它。
- 修改任何规则文件都先问用户，文件清单见根 [AGENTS.md](../../../../AGENTS.md)“何时直接做，何时先问”。用户只是提问时，AI 只回答，不改文件。
- `assertNever` 放在 `packages/protocol/src/assert.ts`，各包共用一份。
- 提交流程写成 skill `.agents/skills/crew-commit/SKILL.md`，`.claude/skills` 是指向 `.agents/skills` 的软链接，做法来自 DSH。测试与写法的规则只写在 `docs/testing.md` 与 `docs/defensive-patterns.md`，skill 只写流程并链接它们，避免同一条规则写在几处。
- `.claude/settings.json` 让推送、硬重置、删除目录等命令执行前询问。规则按命令文本匹配，只覆盖常见写法，例如拦不住 `git -C <路径> push`。

**Agent Note**

- 一份 Note 只负责一个主题。分多步实施的计划写成路线图 Note（[重写的路线图](../../proposed/architecture/2026-10-04-typescript-rewrite.md)），每一步的决策另写 Note，实现后改为 implemented。
- Rust 版的 Note 放在 `.agents/notes/legacy/`，状态行是 `Status: legacy`，只作参考，可以重新讨论。
- 只为会约束后续工作的决策写 Note。界面的视觉方向会约束以后的每个页面，所以有[私聊界面](../feature/2026-10-04-direct-chat-ui.md)这份 Note。
- Note 的“决策”一节只写选了什么，并链接设计文档；数字与默认值只写在设计文档里。
- 讨论要写进 Note 的方案时，Note 跟着讨论更新，不在讨论结束后凭记忆补写。做法见 [Agent Notes 的 README](../../README.md)“讨论中记录”。

**检查**

- 检查放在 `scripts/`，每个都有测试证明它拒绝违规输入，`pnpm lint` 运行它们。DSH 用 `dsh:scripts/verify-*.ts` 检查文档规则，cumora 用 `cumora:scripts/guard-*.mjs` 检查跨文件一致性。
  - `scripts/verify-docs.ts`：Markdown 相对链接指向存在的文件；行内代码写到的路径存在，引用参考项目的路径写成 `项目:路径`，在该项目的本机目录里检查；Agent Note 的位置与格式；根 `AGENTS.md` 的字数上限。
  - `scripts/verify-code.ts`：禁止 `as unknown`；空的 catch 必须写注释。Biome 的 `noEmptyBlockStatements` 会连空的回调函数一起报，所以没有用它。
  - Biome（`biome.json`）：warning 也让 lint 失败；Server 与 Computer 的源码只允许 `console.error` 与 `console.warn`；protocol 禁止只在 Node 或浏览器一边存在的全局变量。
- 没有做的检查：包的 `exports` 指向源码、测试不放进 `src/`、提交时拦下 `.env`。前两者违反时类型检查或测试会立刻暴露，`.env` 已经被 `.gitignore` 排除。
- git hooks 用 lefthook（`lefthook.yml`），只做快速检查，做法来自 DSH 的 `dsh:lefthook.yml`：
  - pre-commit：对暂存文件运行 Biome，只检查、不修复；`git diff --cached --check`；文档检查与代码约定检查。格式问题用 `pnpm format` 修复。
  - commit-msg：标题是 `<type>(<scope>): <中文描述>`，有正文时标题后空一行（`scripts/verify-commit-msg.ts`）。提交说明的格式写在 skill 里，Agent 不一定会读，所以由钩子保证。
  - pre-push：`pnpm typecheck`。

**测试流程**

测试的分层、写法与命令见 [testing.md](../../../../docs/testing.md)。其中这几条是取舍：

- 每个集成测试文件使用独立的临时数据库。
- 冒烟测试运行 `electron-vite build` 的产物，不运行源码，由主进程同一份 `startRuntime` 启动。做法来自 DSH `dsh:docs/testing.md` 的 “Test the real entry path”。
- 单元与集成测试的假 Engine 实现 Engine 接口（`packages/computer/test/support/fake-engine.ts`）。冒烟测试不往构建产物里注入假 adapter，而是在 `PATH` 中放一个假 opencode：发布的代码里没有只给测试用的开关，做法来自 DSH `dsh:docs/testing.md` 的 “Keep opt-ins out of shipped defaults”。
- 真实模型的 e2e 不进 `pnpm check`。没有指定 `CREW_E2E_MODEL` 或没有登录 OpenCode 时跳过，做法来自 DSH 的真实 API 通道（缺少 key 时自动跳过）。它调用付费模型，运行前先问用户。
- 模型可见的文本用 vitest 文件快照逐字锁定，改动时快照的 diff 进入审查。
- 提交前总是跑全量 `pnpm check`。DSH 的 `dsh-pre-push-checks` 按改动挑最小测试集，因为它的全量测试慢、有 CI 兜底；本仓库的全量检查很快，而且没有 CI。

## 考虑过的方案

**照搬 DSH 的完整文档与检查体系。** DSH 有 71 个 `verify-*` 检查、中英双语文档（`.md`、`.zh.md`、`.i18n.yaml`）、冻结的 Agent Note 归档、每个文件 100% 的覆盖率门槛和录制会话回放的快照。没有采用，因为这些服务于多人协作的大型仓库，维护成本超过单人项目的收益。只取三个检查脚本、git hooks 与代码约定。

**路线图 Note 不拆，只要求已实现的部分也与代码一致。** 改动最少。没有采用：一份 Note 混着计划与几十项决策，一个状态行说不清进度；读一项决策要读完四百行；已实现的部分已经与代码不一致（OpenCode 的参数、session 字段名）。用户认为 proposed、implemented、rejected 三种状态适合一个个小的决策，原来的路线图讨论的东西太多。

**Rust 版的 Note 留在 `implemented/`，在根 `AGENTS.md` 加一条豁免。** 没有采用：占常驻规则的字数，删除 Rust 代码时还要再删这条；放在 `legacy/` 后，状态行本身就说明它只作参考。

**讨论结束后再一次写文档。** 原来的做法。没有采用：用户在使用中发现，讨论时有时问“为什么用这个方案”，有时同意某个方案，讨论完成后才写的文档会漏掉讨论中的细节。长对话还会被压缩，越早的细节越容易丢。

**`assertNever` 在每个需要它的包里各写一份。** 只有 4 行，protocol 也不用放工具函数。没有采用：用户选择共用 `packages/protocol` 中的一份。

**删掉私聊界面的 Note，保持“界面调整不写 Note”。** 没有采用：用户同意把规则改为“只为会约束后续工作的决策写”。

**Note 的“决策”一节照旧写全实现细节与数字。** 没有采用：拆分路线图时，9 份 Note 重复写了子系统页与 architecture.md 中的数字，改一个数字要改两处，路径检查发现不了。

**pre-push 运行 `pnpm check`。** 没有 CI 时，它保证推送的代码通过了测试。暂时没有采用：提交基本经过 crew-commit，提交前已经运行过；每次推送都要多等一次全量检查，Docker 没开时推送会失败。经常手动提交时再加。

## 后果

- `CLAUDE.md` 是指向 `AGENTS.md` 的软链接，两种工具读到同一份规则。
- 每个检查脚本都有测试证明它拒绝违规输入（`scripts/` 下的 `*.test.ts`）。
- 没有 CI：手动提交并跳过 `pnpm check` 时，测试不会在推送前运行。
- 改规则时要同时改检查脚本与它的测试。
- 讨论中更新 Note 会让每次回复多一步，换来讨论的细节不丢。
