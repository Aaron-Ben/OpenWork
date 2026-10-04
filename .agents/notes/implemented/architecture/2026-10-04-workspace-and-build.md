# Agent Note: workspace、包划分与构建

Status: implemented

## 问题

Crew 的代码分属 Electron 主进程、界面、Server、Computer 与 `crew` 命令，它们要共用协议类型。需要决定怎样分包、包之间怎样引用、怎样构建与开发，并且让 Server、Computer 与 `crew` 能用 Electron 自带的 Node 运行。总体计划见[重写的路线图](../../proposed/architecture/2026-10-04-typescript-rewrite.md)。

## 决策

- 四个包，由 pnpm workspace 管理：`packages/protocol`、`packages/server`、`packages/computer`、`apps/desktop`。职责与依赖方向见 [architecture.md](../../../../docs/architecture.md) 第 2 节。
- 沙箱代码与 `crew` 命令只有 Computer 使用，放在 `packages/computer` 内，不单独成包。`crew` 是它的第二个入口 `packages/computer/src/shim/main.ts`。
- 包之间直接引用源码：每个包的 `package.json` 用 `"exports": { ".": "./src/index.ts" }`，没有构建步骤。做法来自 raft 的 `raft:packages/shared/package.json`（`main` 指向 `src/index.ts`）。
- 根目录 `tsconfig.base.json` 保存共用的严格选项，每个包的 tsconfig 继承它。raft 每个包写一份完整的 tsconfig，没有共用部分。
- `apps/desktop` 有两份 tsconfig：`apps/desktop/tsconfig.node.json` 覆盖主进程与 preload（Node 类型），`apps/desktop/tsconfig.web.json` 覆盖界面（DOM 类型与 JSX）。界面代码因此无法通过类型检查使用 Node API。
- electron-vite 用一份配置 `apps/desktop/electron.vite.config.ts` 构建主进程、preload 与界面。`pnpm dev` 运行 `electron-vite dev --watch`：界面热更新，主进程、preload、Server、Computer 或 `crew` 的源码改动后重新构建并重启 Electron。不加 `--watch` 时主进程不会重新构建。
- Server、Computer 与 `crew` 是主进程配置的额外入口（`rollupOptions.input`），与主进程一起构建到 `out/main/`。主进程构建使用 `externalizeDeps: false`，把全部依赖打包进产物；否则 workspace 包会在运行时按 TS 源码加载而失败。Server 的迁移目录随构建复制到 `out/main/drizzle`。
- preload 构建为 CommonJS（`index.cjs`），启用沙箱的窗口只能加载这种格式。
- `pg` 把 `pg-native` 声明为可选的 peer 依赖。没有安装时，Vite 在开发模式下用一个加载即抛错的模块代替它，所以配置中的插件把它解析成空模块。
- 主进程在 `ELECTRON_RENDERER_URL` 存在时加载开发服务器。cumora 用同一个环境变量（`cumora:electron/main.cjs`），由 `concurrently` 与 `wait-on` 拼出开发模式。
- Node 与 pnpm 的版本由 `.node-version` 与 `package.json` 的 `packageManager` 固定，raft 用同样的方式。根包名是 `crew`，各包是 `@crew/protocol`、`@crew/server`、`@crew/computer` 与 `@crew/desktop`。
- `apps/desktop/package.json` 设置 `"productName": "Crew"`，应用数据目录因此是 `~/Library/Application Support/Crew`。

## 考虑过的方案

**DSH 式 project references。** DSH 的 `dsh:tsconfig.base.json` 启用 `composite`，每个包先构建声明文件到 `lib/types`，再由 `references` 声明依赖。没有采用：它服务于五十多个包、npm 发布与增量构建，四个不发布的包用不上，还要处理过期的构建产物。

**单个 package。** cumora 用一个 package 加 `@/*` 路径别名。没有采用：渲染进程可以直接导入 Server 的内部代码，也与已经确定的包划分矛盾。

**自己组合 tsup、Vite、concurrently 与 wait-on。** 每一步都可见，tsup 也能打包 Server 与 Computer。没有采用：主进程改动后关闭旧 Electron、启动新 Electron 的逻辑要自己写，这正是 electron-vite 已经处理的部分。

## 后果

- 改一个包不需要先构建依赖它的包，类型检查与测试直接读源码。
- Server、Computer 与 `crew` 运行时必须是打包后的产物，冒烟测试因此先执行 `electron-vite build`，见 [testing.md](../../../../docs/testing.md)。
- 构建配置里有两处针对依赖的特殊处理（`pg-native` 与 `externalizeDeps`）。升级 electron-vite 或 `pg` 时，要确认它们仍然需要。
