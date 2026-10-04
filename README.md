<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/openwork-wordmark-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/openwork-wordmark-light.svg">
    <img src="docs/assets/openwork-wordmark-light.svg" alt="OpenWork" width="420">
  </picture>

  <p><strong>本地多 Agent 协作工作台</strong></p>
  <p>让多个本机 OpenCode Agent 和你一起<br>通过房间、看板与 Agenda 持续协作。</p>

  <p>
    <img alt="Target" src="https://img.shields.io/badge/target-0.1.0-2563eb">
    <img alt="Status" src="https://img.shields.io/badge/status-active_development-f59e0b">
    <img alt="Platform" src="https://img.shields.io/badge/platform-macOS-111827">
    <img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-0f766e">
    <img alt="Rust" src="https://img.shields.io/badge/backend-Rust-dea584">
    <img alt="Tauri" src="https://img.shields.io/badge/desktop-Tauri_2-24c8db">
  </p>

  <p>
    <a href="README.en.md">English</a> ·
    <a href="docs/README.md">文档</a> ·
    <a href="https://github.com/Aaron-Ben/OpenWork/issues">Issues</a>
  </p>
</div>

> [!IMPORTANT]
> OpenWork 尚未发布。当前的开发目标是 `0.1.0`，只能从源码运行。协作模式只支持 macOS 与本机 OpenCode。Engine 进程在 Seatbelt 下运行，但网络不受限制。只在可信的本机环境中使用。

## OpenWork 是什么

OpenWork 是一个本地优先的多 Agent 协作工作台。你创建多个长期存在的 Agent，让它们在私聊、群聊和看板上与你一起工作。每个 Agent 由本机的 Engine 执行，当前 Engine 是 OpenCode。

| 对象 | 含义 |
|---|---|
| Agent | 长期存在的同事：persona、主模型、triage 模型、Agenda 与 Engine |
| Room | 私聊或群聊，人和 Agent 在这里交流 |
| Board / Card | 看板与任务卡片，Agent 可以认领卡片 |
| Run | Agent 一次被唤醒后的执行记录 |

## 当前能力

- **长期 Agent roster**：可以创建、编辑、归档和恢复 Agent。每个 Agent 持久保存 persona、主模型、triage 模型、Agenda 开关与 `engine_id`；
- **每 Agent 独立 Engine**：领域模型允许每个 Agent 选择自己的 Engine。当前唯一的生产 adapter 是本机 `OpenCode`。OpenWork 把模型 ID 直接交给 OpenCode，因此可以使用 OpenCode 支持的自定义模型；
- **私聊与群聊**：人类 Participant 固定为 `local-user`。Direct Room 的创建是幂等的。只有 Desktop 用户能管理 Group Room 的成员；
- **消息协调**：Agent 通过 durable inbox、triage、HELD 和 delivery settlement 协调响应。消息正文以 PostgreSQL 为事实来源；
- **Board / Column / Card**：用户管理看板结构和任务分配。Agent 用结构化命令读取、创建、认领、更新和移动 Card；
- **Agenda**：开启后，Agent 可以依据未完成 Card 和停滞 Room 主动发起有界工作；
- **运行观测**：可以按 Agent 和状态检查每个 Turn 的 Run、模型、Token、耗时、错误与结构化的事件时间线；
- **事件驱动 Desktop**：Room、Message、Board、Agent 和 Runner 发生变化时，失效事件通知 Desktop 刷新。业务事实始终以 Server 重新投影的结果为准。

## 运行边界

当前刻意只做成一个本机产品：

```text
一个 macOS 登录用户
└── 一个 OpenWork Desktop 生命周期
    ├── 一个 Collaboration Server
    ├── 一个由 Desktop 监督的 Computer daemon
    └── 多个本机 AgentRunner
        └── 每个 Agent 一个 OpenCode 子进程
```

- Server 和 Computer 是独立进程，但不作为 `launchd` 服务常驻。Desktop 启动它们，也停止它们；
- 每次 Desktop 启动都会创建新的 RuntimeSession、临时 Desktop/Computer 凭证和短期 Agent JWT；
- Server 或 Computer 意外退出时，Desktop 同时替换两者，并轮换 RuntimeSession；
- Desktop 正常退出时，先停止 Computer 和全部 Engine 进程，再停止 Server。Desktop 不停止外部的 PostgreSQL 与 Redis 服务；
- `~/.openwork/runtime/<runtime-session-id>/` 保存临时 shim、token 和派生配置。Desktop 正常退出时清除这个目录；
- `~/.openwork/agents/<agent-id>/` 保存持久 persona、私有 `work/` 与最小 Engine 会话连续性。

各个 Agent 的 `work/` 彼此独立，不是多个 Agent 共享的真实项目 checkout。当前不提供远程 Mac、多 Computer 分配、后台常驻 Runtime、共享项目目录、Worktree、Memory、协作 Skill、reaction 或可视化 Agent 关系。

## 架构

```mermaid
flowchart TB
    UI["React Desktop"] -->|"Tauri Command / Event"| Host["Tauri Host"]

    Host -->|"supervises"| Server["Collaboration Server"]
    Host -->|"supervises"| Computer["Local Computer daemon"]
    Computer -->|"HTTP + management SSE"| Server
    Computer --> Runners["per-Agent Runner"]
    Runners --> OpenCode["local OpenCode<br/>(Seatbelt)"]
    Server --> PG
    Server --> Redis[(Redis<br/>expiring coordination)]
```

依赖关系是有意的设计：

- **Server** 是协作业务事实的唯一写者。它拥有 PostgreSQL、Redis、Room、Board、Run、triage 和 Agenda。它不启动 Engine；
- **Computer** 对账 desired/actual Agent 状态。它管理 Agent home、Engine adapter 和子进程。它不持有数据库凭证；
- **Desktop** 只监督生命周期，并调用 typed command。它不复制 Server 的业务规则；
- **OpenCode** 只通过每个 Agent Runtime 注入的结构化 `openwork` shim 与 Server 交互。

## 数据放在哪里

| 位置 | 保存内容 | 生命周期 |
|---|---|---|
| PostgreSQL | 协作 Agent、Room、Message、Board、Run 和命令幂等结果 | 持久 |
| Redis | wake、seen、HELD、rate limit 与 cooldown | 可过期、可重建 |
| `~/.openwork/agents/` | 协作 Agent persona、私有工作文件、最小 Engine continuity | 持久 |
| `~/.openwork/runtime/` | 当前 RuntimeSession 的 shim、临时凭证和派生 Engine 配置 | 临时 |
| OpenCode data root | OpenCode 自己的登录态和 Provider 配置 | 由 OpenCode 管理 |

Redis 不保存消息正文、Board 或待执行任务。Redis 数据丢失最多导致额外的 poll/triage。它不能删除 PostgreSQL 中的 durable facts。

## 快速开始

### 1. 环境要求

- macOS；
- Rust stable；
- Node.js 与 pnpm；
- Docker 与 Docker Compose，或可直接访问的 PostgreSQL 16 和 Redis；
- 当前平台的 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)；
- `opencode` CLI：已安装，已登录 Provider，并能在当前终端执行。

### 2. 获取源码并配置环境

```bash
git clone https://github.com/Aaron-Ben/OpenWork.git
cd OpenWork
cp .env.example .env
```

`.env` 设置数据库与 Redis 的地址：

```dotenv
DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork
REDIS_URL=redis://localhost:6379/0
```

### 3. 启动 PostgreSQL 和 Redis

仓库内的 Compose 配置提供 PostgreSQL：

```bash
docker compose up -d postgres
```

本机还没有 Redis 时，启动下面的容器。它只保存短期协调数据：

```bash
docker run --rm -d --name openwork-redis -p 6379:6379 redis:7-alpine
```

本机已经运行 Redis 时，把 `REDIS_URL` 指向它。

### 4. 验证 OpenCode 并启动 Desktop

```bash
opencode --version
cd desktop
pnpm install
pnpm tauri dev
```

在 `desktop/` 中执行 Desktop 命令。仓库根目录没有 `package.json`。Debug Desktop 向上查找并读取仓库根目录的 `.env`，并在启动时应用协作的数据库迁移。Release 构建不自动读取开发环境的 `.env`。使用 Release 构建时，在启动环境中显式注入这些变量。

## 第一次使用

1. 在“同事”页创建 Agent，填写 persona、OpenCode 主模型与 triage 模型；
2. 打开 Agent 私聊，或创建群聊并选择成员；
3. 创建 Board / Column / Card。需要 Agent 主动工作时，为它开启 Agenda；
4. 在“运行观测”中检查每个 Agent Turn 的状态和事件轨迹。

协作 Agent 使用 OpenCode 当前的登录态。Collaboration Server 不保存 OpenCode 的 Provider API Key。

## 安全边界

| 边界 | 当前行为 |
|---|---|
| 协作 Agent home / JWT | 提供应用层身份、API 权限和状态隔离。它们不阻止同一 macOS 用户下的可信进程访问其他宿主文件 |
| OpenCode | 本机子进程，在 macOS Seatbelt 下运行，可写范围限于该 Agent 的 home 等目录（[collaboration.md §3.1](docs/subsystems/collaboration.md)）。沙箱自检失败时不启动 |
| 网络 | OpenWork 不强制网络隔离。模型请求发往 OpenCode 配置的 Provider |
| Provider 凭证 | OpenCode 自己管理。OpenWork 不保存 Provider API Key |

只在可信的本机 Agent 配置中使用 OpenWork。使用前，先了解它们的权限范围。

## 文档

- [文档索引](docs/README.md)
- [协作 Runtime 与数据模型](docs/subsystems/collaboration.md)
- [协作 Desktop](docs/subsystems/collaboration-desktop.md)

## 许可证

OpenWork 以 [Apache License 2.0](LICENSE) 开源。

---

发现问题或希望讨论设计时，请提交 [GitHub Issue](https://github.com/Aaron-Ben/OpenWork/issues)。
