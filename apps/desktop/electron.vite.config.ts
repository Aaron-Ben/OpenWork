import { cpSync } from "node:fs";
import { join, resolve } from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "electron-vite";
import type { Plugin } from "vite";

const desktop = import.meta.dirname;
const repoRoot = resolve(desktop, "../..");

/**
 * `pg` 把 `pg-native` 声明为可选的 peer 依赖，只在访问 `pg.native` 时才 require 它（pg/lib/index.js）。
 * 没有安装时，Vite 在开发模式下用一个加载即抛错的模块代替它，Server 因此无法启动。
 * 我们不用原生驱动，所以在 Vite 自己的解析之前（enforce: "pre"）把它解析成空模块。
 */
function stubPgNative(): Plugin {
  const id = "\0pg-native-stub";
  return {
    name: "crew:stub-pg-native",
    enforce: "pre",
    resolveId: (source) => (source === "pg-native" ? id : null),
    load: (source) => (source === id ? "export default null;" : null),
  };
}

/**
 * 把 Server 的迁移目录复制到主进程产物旁的 `drizzle/`，主进程经 `CREW_MIGRATIONS_DIR` 告诉 Server。
 * 跟随实际的输出目录：冒烟测试用 `--outDir` 构建到单独的目录。
 */
function copyMigrations(): Plugin {
  return {
    name: "crew:copy-migrations",
    writeBundle: (options) => {
      if (!options.dir) throw new Error("主进程构建缺少输出目录");
      cpSync(resolve(repoRoot, "packages/server/drizzle"), join(options.dir, "drizzle"), { recursive: true });
    },
  };
}

export default defineConfig(({ mode }) => ({
  main: {
    plugins: [stubPgNative(), copyMigrations()],
    build: {
      // Server、Computer 与 shim 用 Electron 自带的 Node 运行，不能在运行时加载 workspace 包的 TS 源码，
      // 所以把全部依赖打包进产物。
      externalizeDeps: false,
      rollupOptions: {
        input: {
          index: resolve(desktop, "electron/main.ts"),
          server: resolve(repoRoot, "packages/server/src/main.ts"),
          computer: resolve(repoRoot, "packages/computer/src/main.ts"),
          shim: resolve(repoRoot, "packages/computer/src/shim/main.ts"),
        },
      },
    },
  },
  preload: {
    build: {
      rollupOptions: {
        input: { index: resolve(desktop, "electron/preload.ts") },
        // 启用沙箱的窗口只能加载 CommonJS 格式的 preload。
        output: { format: "cjs", entryFileNames: "[name].cjs" },
      },
    },
  },
  // 冒烟测试不打开窗口，`--mode smoke` 时不构建界面。
  ...(mode === "smoke"
    ? {}
    : {
        renderer: {
          root: resolve(desktop, "src"),
          plugins: [react(), tailwindcss()],
          build: {
            rollupOptions: { input: resolve(desktop, "src/index.html") },
          },
        },
      }),
}));
