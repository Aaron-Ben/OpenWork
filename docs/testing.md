# 测试

本文规定测试的分层、原则、写法与运行方式。做法参照 DSH 的 `dsh:docs/testing.md`。检查命令与完成的标准见根 [AGENTS.md](../AGENTS.md)；旧版 Rust 代码的检查见 [legacy-rust.md](legacy-rust.md)。

## 1. 分层

| 层 | 位置 | 内容 | 命令 |
|---|---|---|---|
| 单元 | 各包的 `test/*.test.ts` | 纯函数：界面逻辑、协议解析、Seatbelt 规则生成 | `pnpm test` |
| 集成 | `packages/*/test/`、`apps/desktop/test/` | Server 的 HTTP 与事务；Computer 与 Server 的往返；真实 Seatbelt 下的 OpenCode 适配器（用假 opencode） | `pnpm test` |
| 快照 | `test/__snapshots__/*.md` | 模型可见的文本，见第 4 节 | `pnpm test` |
| 冒烟 | `apps/desktop/test/smoke.e2e.ts` | 构建应用，用主进程同一份 `startRuntime` 启动构建好的 Server、Computer 与 shim；假 opencode 在 Seatbelt 里经 `crew reply` 回复，断言回复落库 | `pnpm test:smoke` |
| 真实模型 | `apps/desktop/test/real-opencode.e2e.ts` | 与冒烟相同的构建产物与启动路径，用本机已登录的 OpenCode 调用真实模型 | `CREW_E2E_MODEL=<模型> pnpm test:e2e` |
| 手动 | `pnpm dev` | 界面：新建 Agent、发消息、看到回复与状态变化 | — |

- `pnpm check` 依次运行 lint、类型检查、`pnpm test` 与 `pnpm test:smoke`，全量约 17 秒。
- 集成与冒烟测试需要 `TEST_DATABASE_URL` 与 `TEST_REDIS_URL`，由 `@crew/server/testing` 的 `testEnv()` 读取，环境变量没有设置时读根目录的 `.env`。缺少时测试直接失败，不跳过。
- 依赖 Seatbelt 的测试只在 macOS 上运行。
- 真实模型测试不进 `pnpm check`。没有指定 `CREW_E2E_MODEL`、没有 `opencode` 或没有登录时整组跳过，模型由运行的人选。改动 Engine 调用或模型可见的行为时运行它，并在汇报中写明结果。

## 2. 原则

- **只替换昂贵或不确定的边界。** 模型与时钟可以替换，数据库、Redis、Seatbelt 与 HTTP 用真实实现。例如 `packages/computer/test/runner.test.ts` 用内存中的真实 Server 应用与临时数据库，只把 Engine 换成按脚本回复的假 Engine。
- **验证真实世界，不相信自我报告。** 断言重新读取数据库或文件，不只看被测对象返回了什么。例如 shim 测试从 Server 读回消息，而不是只看命令的输出。失败分支断言没有副作用：没有写库，没有改文件。
- **走真实入口。** 冒烟测试运行构建产物，由 Electron 以 Node 方式执行，与用户运行的是同一套代码与启动路径。只跑源码测不出打包、模块解析与启动顺序的问题。
- **测试自己拥有资源。** 测试创建的资源由测试释放，失败时也一样，做法见第 5 节。只在单独运行时才通过的测试，是测试本身的缺陷。
- **守卫要能失败。** 新加一个防回归的测试时，先引入那个回归，看到测试失败，再恢复。例如去掉 Runner 的 `PATH` 中 `bin/crew` 所在的目录，冒烟测试就会失败。

## 3. 写法

- 测试名用英文写行为与结果，读起来是一句话，例如 `keeps the messages and reports the reason when the turn fails`。禁止 `test 1`、`works`、`basic`。
- 断言具体结果。用 `toEqual` 断言完整的值，不只断言真值或长度。
- 禁止为了通过而放宽断言，例如把精确比较改成包含、删掉一条断言。
- 等具体条件，不 `sleep` 固定时长。用轮询等到条件成立，超时只作为等待的上限，例如 `apps/desktop/test/support/built-app.ts` 的 `until`。
- 前端不写只断言静态 HTML 的组件测试。逻辑抽成纯函数，写 `.test.ts`，例如 `apps/desktop/test/lib.test.ts`。

## 4. 快照

- 模型可见的文本用 vitest 的 `toMatchFileSnapshot` 逐字锁定，放在包的 `test/__snapshots__/` 中，用 `.md` 扩展名方便阅读。
- 现有快照：`AGENTS.md`（Agent 的常驻规则）、`turn-prompt.md`（每轮输入）、`shim-output.md`（`crew` 的全部输出）。
- 改动这些文本时，快照的 diff 与代码一起审阅。更新快照用 `pnpm --filter <包> exec vitest run -u`，然后逐行看 diff。

## 5. 隔离与资源

vitest 同时运行多个测试文件，`pnpm -r test` 让各包并行；它们共用同一个 PostgreSQL、Redis 与本机端口。每个测试占用的资源都要有私有的分配方式和明确的释放点。

- **数据库：** 每个测试文件用 `createTestDatabase` 建自己的临时数据库，结束时删除。
- **Server：** 优先用 `createTestApp`：真实的路由与数据库，经 `t.fetch` 在内存中调用，不占端口。需要真实端口时监听 `127.0.0.1:0`，在“已监听”之后读取分配到的端口。
- **目录：** 用 `mkdtemp` 建私有的临时目录。不写真实的 `~/.crew`，路径经参数注入。冒烟测试把构建产物、crew 目录都放在自己的临时目录里，同时运行的两次检查互不影响。
- **全局状态：** 优先注入依赖，不改全局，例如 `ServerClient` 的 `fetchFn`、`crew` 的 `CliIo`。必须修改 `process.env`、计时器等时，记下原值，在 `finally` 中恢复。
- **清理：** 资源一创建就注册清理，并等到结束信号：`await runtime.stop()` 等子进程退出，`await` SSE 循环返回。只调用 `abort()` 或 `kill()` 而不等待，清理就没有完成。子进程、流与取消的写法另见 [defensive-patterns.md](defensive-patterns.md)。
- **偶发失败：** 找到原因再修。加长超时、加重试、改成串行、在断言前 `sleep` 都不是修复；重跑后通过也不算修好。

## 6. 什么时候必须加测试

- 新行为：至少覆盖正常路径、边界值与非法输入。
- 修 bug：先写能复现的失败测试，再修。
- 模型可见的文本：在同一个改动里更新快照。
- 新的构建入口或启动路径（`electron.vite.config.ts`、`apps/desktop/electron/`、各包的 `main.ts`）：冒烟测试要能覆盖它。
- 沙箱与路径规则：覆盖不存在的路径、符号链接、带空格与非 ASCII 的路径。

## 7. 运行

```bash
pnpm check                                                        # 提交前：lint、类型检查、测试与冒烟测试
pnpm test                                                         # 单元、集成与快照
pnpm test:smoke                                                   # 冒烟测试
CREW_E2E_MODEL=deepseek/deepseek-flash pnpm test:e2e              # 真实模型，不进 pnpm check
pnpm --filter @crew/server test                                   # 一个包的测试
pnpm --filter @crew/computer exec vitest run test/shim.test.ts    # 一个测试文件
```

开发时先运行改动相关的测试；提交前运行 `pnpm check`，步骤见 [crew-commit](../.agents/skills/crew-commit/SKILL.md)。
