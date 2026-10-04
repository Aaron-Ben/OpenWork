# Crew

本文只放每个会话都需要的常驻规则。其他文档见 [docs/README.md](docs/README.md)，这里不重复。

## 项目目的与优先级

- 这是求职作品，目标岗位是全栈与 AI 应用工程师。产品是本地多 Agent 协作工作台。
- 优先级从高到低：**能跑 → 能演示 → 能讲清楚设计取舍 → 架构整洁**。
- 判断一项工作值不值得做，先问：它能不能让面试官更快看到效果，或让我更好地讲清楚一个设计。都不能时，先不做。
- 项目正在参考 cumora 与 raft，用 TypeScript 从零实现新版本 Crew，设计见 [Agent Note](.agents/notes/proposed/architecture/2026-10-04-typescript-rewrite.md)。开发期间不给 Rust 代码加功能，只修影响旧版本运行的问题。

## 运行与检查

启动 Crew：先运行 `docker compose up -d --wait` 启动 PostgreSQL 与 Redis，再在根目录运行 `pnpm dev`。数据库地址来自根目录的 `.env`，见 [docs/local-services.md](docs/local-services.md)。

- **改动后**：`pnpm lint`、`pnpm typecheck`，以及与改动相关的测试。
- **提交前**：`pnpm check`，即 lint、类型检查、测试与冒烟测试。用户说“只做语法检查”时，只跑 lint 与类型检查，并在汇报中写明没有跑测试。提交流程见 [crew-commit](.agents/skills/crew-commit/SKILL.md)。
- lefthook 在提交时检查暂存文件的格式与空白，在推送前运行类型检查。
- 改旧版 Rust 代码前，先看 [docs/legacy-rust.md](docs/legacy-rust.md)。

代码约定见 [packages/AGENTS.md](packages/AGENTS.md)，测试规则见 [docs/testing.md](docs/testing.md)。

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

## 任务与完成标准

- 开始一项任务前写清“完成”是什么：哪些检查通过、能演示什么。
- 分多块的长任务在 `.agents/tasks/` 建任务文件：每块写完成标准，做完打勾。进度以文件为准，不以聊天记录为准。

## 何时直接做，何时先问

不需要用户决定的步骤直接做，在汇报中写明选了什么、为什么。把进度和下一步放在同一条消息里，不用“要我继续吗”结尾。

以下情况先停下来，写清现状、选项和建议：

- **用户能看到的界面或行为会改变**，包括删掉一个入口或一项显示；
- 数据模型、协议或包边界的设计；
- 新增或删除包、依赖；
- 设计与现实冲突、有歧义或做不到；
- 任何提交、推送、合并；
- 破坏性操作：删除数据或数据库、`git reset --hard`、force push、删除不是本次创建的文件；
- 改动仓库以外的内容；
- 修改本文件（`AGENTS.md`）。`CLAUDE.md` 是指向它的软链接，改真实文件。

开始实现一个功能前，把属于上面几类的决策点一次列出，每个给出推荐；其余实现细节按推荐直接做。用户说“暂时不改”时，只回答问题。

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
