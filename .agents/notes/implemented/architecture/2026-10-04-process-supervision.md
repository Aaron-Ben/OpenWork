# Agent Note: 主进程监管 Server 与 Computer

Status: implemented

## 问题

Crew 有三个进程：Electron 主进程、Server 与 Computer，这是[路线图](../../proposed/architecture/2026-10-04-typescript-rewrite.md)中已定的决策。raft 与 cumora 的 Electron 主进程都不启动本地 Server，主进程怎样启动、监管与停止 Server 和 Computer 需要自己设计：凭证怎样传递，怎样知道子进程就绪，子进程崩溃或主进程被强制结束时怎样处理。

## 决策

- 主进程用 `child_process.spawn` 运行 Electron 可执行文件，设置 `ELECTRON_RUN_AS_NODE=1`，参数是 Server 或 Computer 的入口 JS。测试中用 `node` 运行同样的入口，监管代码不变。代码在 `apps/desktop/electron/runtime.ts` 与 `apps/desktop/electron/child.ts`。
- 启动握手：主进程向子进程的 stdin 写一行 JSON（bootstrap），子进程就绪后向 stdout 写一行 JSON（ready）。凭证不经过命令行参数与环境变量；Server 的随机端口由 ready 带回。消息的 schema 在 `packages/protocol/src/runtime.ts`。这沿用 Rust 版的做法（代码已在第 9 步删除，原文见 `git show dd8779b:crates/openwork-collab/src/protocol/desktop.rs`）。
- 主进程启动时读取根目录的 `.env`，生成 RuntimeSession ID 与 Desktop、Computer 两个随机凭证。
- 先启动 Server，再启动 Computer，两者都 ready 后才创建窗口。启动 Computer 时从环境变量中删除 `DATABASE_URL` 与 PostgreSQL 相关的变量（`computerEnv`）：Computer 只经 Server 的 HTTP 接口访问数据。Rust 版的做法见 `git show dd8779b:desktop/src-tauri/src/collab_client.rs` 的 `spawn_child`（代码已在第 9 步删除）。
- 子进程超时没有报告 ready 就按启动失败处理。停止时先停 Computer 再停 Server，每个先发 SIGTERM，宽限期过后仍未退出就发 SIGKILL。
- 主进程给子进程 stderr 的每一行加上 `[Server]` 或 `[Computer]` 前缀，转发到自己的 stderr，并保留末尾一段用于报错。
- 启动失败，或 Server、Computer 意外退出时，主进程停止整组进程，用系统错误对话框显示原因与 stderr 末尾，然后退出应用，不自动重启。
- Server 与 Computer 读完 bootstrap 后继续读取 stdin。stdin 结束说明主进程已经退出（包括被 SIGKILL），它们随之退出，不留下孤儿进程。
- 同一时间只运行一个 Crew：主进程使用 Electron 的单实例锁，第二个实例直接退出，已有的窗口切到前台。否则第二个实例会删掉第一个实例的本次运行目录，两个 Computer 还会处理同一批消息。raft 与 cumora 都这样做（`raft:apps/raft-desktop-electron/src/app/index.ts`、`cumora:electron/main.cjs`）。
- 只支持开发模式：没有 `ELECTRON_RENDERER_URL` 时主进程报错退出。

当前的行为与时限见 [architecture.md](../../../../docs/architecture.md) 第 1 节。

## 考虑过的方案

**用 `utilityProcess` 启动 Server 与 Computer。** 由 Electron 管理子进程。没有采用：入口依赖 Electron 的 API，测试也必须启动 Electron。

**重新启动自身并带上角色参数。** raft（`raft:apps/raft-desktop-electron/src/app/index.ts` 的 `findHeadlessMode`）与 Tauri 版都这样做。没有采用：入口必须在任何界面代码之前识别角色参数，raft 的注释记录了漏掉参数时子进程打开第二个窗口的缺陷。

## 后果

- 测试与生产使用同一套监管代码：测试用 `node` 运行入口，冒烟测试直接调用 `startRuntime`。
- 凭证只出现在 stdin 上，`ps` 看不到。主进程被强制结束时，子进程靠 stdin 关闭得知并退出。
- 应用启动时，窗口要等 Server 与 Computer 都就绪后才出现。
- 依赖 Electron 的 `runAsNode` fuse：关闭这个 fuse 的打包配置会让 Server、Computer 与 `crew` 都无法启动。
- 停止的时限要逐级小于上一级：Server 关闭时等待正在进行的请求的时间（`packages/server/src/serve.ts`），要短于主进程给它的宽限期。做法见 [defensive-patterns.md](../../../../docs/defensive-patterns.md)“停止要在宽限期内完成”。
