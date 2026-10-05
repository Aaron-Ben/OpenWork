import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { app, BrowserWindow, dialog, ipcMain, nativeTheme, shell } from "electron";
import { ChildStartError } from "./child";
import { MEMORY_CHANNEL, type RendererRuntime, RUNTIME_CHANNEL } from "./contract";
import { readAgentMemory } from "./memory";
import { confineNavigation } from "./navigation";
import { type Runtime, startRuntime } from "./runtime";

// Electron 主进程：启动 Server 与 Computer，两者 ready 后打开窗口；任一意外退出时停止整组并报错退出。

let runtime: Runtime | undefined;
let window: BrowserWindow | undefined;
let quitting = false;

/** 弹出错误对话框，停止整组进程，然后退出应用。 */
async function fail(title: string, detail: string): Promise<void> {
  dialog.showErrorBox(title, detail);
  quitting = true;
  await runtime?.stop();
  app.exit(1);
}

function describeError(error: unknown): string {
  if (error instanceof ChildStartError) {
    return error.stderr ? `${error.message}\n\n${error.stderr}` : error.message;
  }
  return error instanceof Error ? error.message : String(error);
}

async function start(): Promise<void> {
  const rendererUrl = process.env.ELECTRON_RENDERER_URL;
  if (!rendererUrl) {
    throw new Error("第 1 步只支持用 pnpm dev 启动（缺少 ELECTRON_RENDERER_URL）");
  }

  const rendererOrigin = new URL(rendererUrl).origin;

  // 开发模式下 app.getAppPath() 是 apps/desktop，.env 在仓库根目录。
  process.loadEnvFile(resolve(app.getAppPath(), "../../.env"));

  const crewRoot = join(homedir(), ".crew");
  runtime = await startRuntime({
    executable: process.execPath,
    serverEntry: join(import.meta.dirname, "server.js"),
    computerEntry: join(import.meta.dirname, "computer.js"),
    migrationsDir: join(import.meta.dirname, "drizzle"),
    shimEntry: join(import.meta.dirname, "shim.js"),
    crewRoot,
    env: process.env,
    rendererOrigin,
  });

  runtime.onCrash((crashed) => {
    if (quitting) return;
    void fail("Crew 已停止", `${crashed.name} 意外退出。\n\n${crashed.stderrTail()}`);
  });

  const rendererRuntime: RendererRuntime = {
    serverUrl: runtime.serverUrl,
    desktopToken: runtime.desktopToken,
  };
  ipcMain.on(RUNTIME_CHANNEL, (event) => {
    event.returnValue = rendererRuntime;
  });
  ipcMain.handle(MEMORY_CHANNEL, (_event, agentId: unknown) => readAgentMemory(crewRoot, agentId));

  window = new BrowserWindow({
    width: 1100,
    height: 720,
    minWidth: 760,
    minHeight: 480,
    // 系统的红黄绿按钮放进侧栏顶部；背景色与侧栏一致，页面加载前不闪白。
    titleBarStyle: "hiddenInset",
    trafficLightPosition: { x: 18, y: 18 },
    backgroundColor: nativeTheme.shouldUseDarkColors ? "#0a0c0e" : "#f1f2ee",
    webPreferences: {
      preload: join(import.meta.dirname, "../preload/index.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  confineNavigation(window.webContents, rendererOrigin, (url) => shell.openExternal(url));
  await window.loadURL(rendererUrl);
}

// 只运行一个 Crew：第二个实例会删掉第一个实例的本次运行目录，两个 Computer 还会处理同一批消息。
// 拿不到锁的实例直接退出，已有的实例把窗口切到前台。raft 与 cumora 都这样做
// （raft:apps/raft-desktop-electron/src/app/index.ts、cumora:electron/main.cjs）。
if (!app.requestSingleInstanceLock()) {
  quitting = true;
  app.quit();
} else {
  app.on("second-instance", () => {
    if (!window || window.isDestroyed()) return;
    if (window.isMinimized()) window.restore();
    window.show();
    window.focus();
  });

  app
    .whenReady()
    .then(start)
    .catch((error: unknown) => fail("Crew 无法启动", describeError(error)));
}

app.on("before-quit", (event) => {
  if (quitting || !runtime) return;
  event.preventDefault();
  quitting = true;
  void runtime.stop().then(() => app.quit());
});

app.on("window-all-closed", () => app.quit());
