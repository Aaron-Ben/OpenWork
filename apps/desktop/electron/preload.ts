import { contextBridge, ipcRenderer } from "electron";
import { MEMORY_CHANNEL, type RendererBridge, type RendererRuntime, RUNTIME_CHANNEL } from "./contract";

// 页面加载前向主进程索取一次 Server 地址与 Desktop 凭证，只把这两个值暴露给页面。
// 凭证不经过命令行参数，避免出现在 `ps` 的输出里。另外暴露读 Agent 记忆的方法，由主进程读本机文件。
const runtime: RendererRuntime = ipcRenderer.sendSync(RUNTIME_CHANNEL);
const bridge: RendererBridge = {
  ...runtime,
  readMemory: (agentId) => ipcRenderer.invoke(MEMORY_CHANNEL, agentId),
};

contextBridge.exposeInMainWorld("crew", bridge);
