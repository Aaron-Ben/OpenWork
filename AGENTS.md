# OpenWork

本文只放每个会话都需要的常驻规则。产品与架构的事实见 [docs/architecture.md](docs/architecture.md)，这里不重复。

## 项目目的与优先级

- 这是求职作品，目标岗位是全栈与 AI 应用工程师。产品是本地多 Agent 协作工作台。
- 优先级从高到低：**能跑 → 能演示 → 能讲清楚设计取舍 → 架构整洁**。
- 判断一项工作值不值得做，先问：它能不能让面试官更快看到效果，或让我更好地讲清楚一个设计。都不能时，先不做。
- 项目正在用 TypeScript 重写，设计见 [Agent Note](.agents/notes/proposed/architecture/2026-10-04-typescript-rewrite.md)。移植期间不给 Rust 代码加功能，只修影响旧版本运行的问题。

## 运行与检查

启动需要 PostgreSQL、Redis、已登录的 `opencode` CLI，以及根目录的 `.env`。步骤见 [README 快速开始](README.md#快速开始)。

检查分两级：

- **日常改动**：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`pnpm --dir desktop typecheck`。
- **提交或合并前**：设置 `TEST_DATABASE_URL` 与 `TEST_REDIS_URL` 后运行 `scripts/check.sh`。用户说“只做语法检查”时，只跑日常一级，并在汇报中写明没有跑测试。

前端不写只断言静态 HTML 的组件测试。逻辑抽成纯函数，写 `.test.ts`。测试细节见 [docs/testing.md](docs/testing.md)。

## 设计原则

- 选择能完整满足当前需求的最简单实现。不加投机的抽象、配置和间接层。
- 分层增长。先做出能端到端工作的最小版本，再逐项加能力。不要用能工作的产品换取未完成的复杂度。
- 不保留向后兼容。删除过时的路径，不加兼容层或回退路径。
- 优先用项目里已有的依赖，其次用成熟的库。没有明确理由时，不重新实现常见功能。
- 一个模块只有一个使用方时，并进使用方，不单独拆 crate 或包。
- 能机械检查的规则，写成会被执行的检查脚本，不只写在文档里。
- **原则冲突时，以“项目目的与优先级”为准。**

## 决策记录

- 只为影响长期结构的决策写 Agent Note（`.agents/notes/`），格式见 [README](.agents/notes/README.md)。单个功能、界面调整不写。
- 不重新讨论 implemented Note 的决策。发现新事实时，先说明新事实，再提出重议。
- 改动 Note 提到的路径、符号或默认值时，在同一个改动里更新它。

## 何时直接做，何时先问

不需要用户决定的步骤直接做。把进度和下一步放在同一条消息里，不用“要我继续吗”结尾。

以下情况先停下来，写清现状、选项和建议：

- **用户能看到的界面或行为会改变**，包括删掉一个入口或一项显示；
- 新增或删除 crate、包、依赖；
- 设计与现实冲突、有歧义或做不到；
- 开始实现一个功能前：先列出其中的实现决策点，逐个讨论定下来再写代码；
- 任何提交、推送、合并；
- 改动仓库以外的内容；
- 修改本文件（`AGENTS.md`）。`CLAUDE.md` 是指向它的软链接，改真实文件。

用户说“暂时不改”时，只回答问题。

## 提交

- 只在用户明确同意后提交或推送。一次同意只对那一次操作有效。
- 提交信息用中文，格式为 `<type>(<scope>): <描述>`。
- 提交前检查暂存区：不提交 `.env`、密钥、`node_modules`、`target/`。

## 参考项目

| 名称 | 路径 | 参考什么 |
|---|---|---|
| Cumora | `/Volumes/Extreme SSD/Code/cumora` | 协作规则、Engine adapter、triage |
| Raft | `/Volumes/Extreme SSD/Code/raft-source` | TS monorepo、runtime driver、shim CLI |
| DSH | `/Volumes/Extreme SSD/Code/deepseek-harness` | 文档分层、Agent Note、测试规则、检查脚本 |

- 参考项目的做法只作证据，不直接照搬。引用时说明它为什么这样做、在本项目是否成立，再给出选项与建议。
- 引用参考项目的做法时，给出文件路径。
- 没有读过源码、只凭印象或文档写出的内容，标为“未确认”。
- 写“来自 X”的内容，必须能在 X 的源码里找到。

## 写作与汇报

- 用中文回答。代码注释的语言跟随所在文件。
- 写文档：短句，主动语态，一段一个主题。文档分层见 [docs/AGENTS.md](docs/AGENTS.md)。
- 改了代码或文件后，按顺序汇报：**需要你决定**、**改了什么**、**发现了什么**、**未确认**。没有内容的小节直接省略。纯讨论的问题不用这个格式。
