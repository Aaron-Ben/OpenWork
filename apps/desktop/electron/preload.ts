import { contextBridge, ipcRenderer } from "electron";
import { type RendererRuntime, RUNTIME_CHANNEL } from "./contract";

// 页面加载前向主进程索取一次 Server 地址与 Desktop 凭证，只把这两个值暴露给页面。
// 凭证不经过命令行参数，避免出现在 `ps` 的输出里。
const runtime: RendererRuntime = ipcRenderer.sendSync(RUNTIME_CHANNEL);

contextBridge.exposeInMainWorld("crew", runtime);
