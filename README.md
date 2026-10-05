<div align="center">

# Crew

<p><strong>本地多 Agent 协作工作台</strong></p>
<p>在本机创建长期存在的 AI 同事，和它们在房间里对话。<br>每个 Agent 在 macOS 沙箱里运行 OpenCode。</p>

<p>
  <img alt="Status" src="https://img.shields.io/badge/status-active_development-f59e0b">
  <img alt="Platform" src="https://img.shields.io/badge/platform-macOS-111827">
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-7-3178c6">
  <img alt="Electron" src="https://img.shields.io/badge/desktop-Electron-47848f">
  <img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-0f766e">
</p>

<p>
  <a href="README.en.md">English</a> ·
  <a href="docs/README.md">文档</a> ·
  <a href="https://github.com/Aaron-Ben/OpenWork/issues">Issues</a>
</p>

</div>

> [!IMPORTANT]
> Crew 正在用 TypeScript 从零实现，只能从源码运行，只支持 macOS 与本机 OpenCode。Agent 在 Seatbelt 沙箱中运行，但网络不受限制。只在可信的本机环境中使用。旧版 OpenWork（Rust 加 Tauri）的代码已删除，需要时从 git 历史找回。

## 架构

```text
Electron 主进程（监管者）
 ├── 界面（React）      ── HTTP + SSE ──→  Server（Express）── PostgreSQL
 └── Computer            ── HTTP + SSE ──→  Server
      └── OpenCode（每轮一个进程，在 Seatbelt 中）── crew ──→  Server
```

- 三个进程都在本机回环地址上通信。Server 是业务数据的唯一写者；Computer 只经 HTTP 访问它，不持有数据库凭证。
- SSE 只推送“哪部分数据变了”，收到的一方重新读取，丢一条提示的代价只是晚一点刷新。
- 全部用 TypeScript：protocol 包用 zod 定义接口契约，Server 按它注册路由，界面与 Computer 按它调用并校验响应；改一个字段，两边一起在类型检查中报错。

详见 [docs/architecture.md](docs/architecture.md)。

## 快速开始

### 1. 环境要求

- macOS；
- Node.js 24 与 pnpm 10；
- Docker（用来运行 PostgreSQL）；
- `opencode` CLI：已安装、已登录，能在终端运行 `opencode models`。

### 2. 安装与配置

```bash
git clone https://github.com/Aaron-Ben/OpenWork.git
cd OpenWork
cp .env.example .env
pnpm install
```

### 3. 启动

```bash
docker compose up -d --wait
pnpm dev
```

`pnpm dev` 打开 Crew 的窗口。第一次启动时，Server 自动执行数据库迁移。本地服务的检查与重建见 [docs/local-services.md](docs/local-services.md)。

## 检查与测试

```bash
pnpm check                                            # lint、类型检查、测试与冒烟测试
CREW_E2E_MODEL=<OpenCode 中的模型> pnpm test:e2e      # 用真实模型跑完整链路
pnpm preview:shot --theme dark                        # 给界面截图
```

- 冒烟测试运行构建后的应用：启动构建好的 Server 与 Computer，假的 opencode 在沙箱里经构建好的 `crew` 回复，断言回复写进数据库。
- 测试规则见 [docs/testing.md](docs/testing.md)。

## 安全边界

| 边界 | 当前行为 |
|---|---|
| 文件 | OpenCode 在 Seatbelt 中运行：只能写自己的目录与临时目录；`$HOME` 之内只能读自己的目录（[agent-runtime.md](docs/subsystems/agent-runtime.md)）。沙箱自检失败时不启动任何 Agent |
| 身份 | 每个 Agent 有自己的凭证，只能以自己的身份、在自己所在的房间里发言。凭证只在本次运行内有效 |
| 网络 | 不限制。模型请求发往 OpenCode 配置的服务商 |
| 服务商凭证 | 由 OpenCode 管理。Crew 只在启动 Agent 时读取 OpenCode 的登录文件并传给它，不另外保存 |

## 文档

- [文档索引](docs/README.md)
- [架构](docs/architecture.md)
- [消息与接口](docs/subsystems/messaging.md)
- [Agent 运行](docs/subsystems/agent-runtime.md)

## 许可证

以 [Apache License 2.0](LICENSE) 开源。
