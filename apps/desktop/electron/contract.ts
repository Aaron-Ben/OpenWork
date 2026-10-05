// 主进程、preload 与界面三方共用的约定。界面只引用其中的类型。

/** preload 向主进程索取运行期信息的 IPC 通道。 */
export const RUNTIME_CHANNEL = "crew:runtime";

/** 页面向主进程读 Agent 记忆的 IPC 通道。 */
export const MEMORY_CHANNEL = "crew:memory";

/** 页面经 `window.crew` 读到的运行期信息：Server 地址与 Desktop 凭证。 */
export interface RendererRuntime {
  serverUrl: string;
  desktopToken: string;
}

/** 一个 Agent 的 `MEMORY.md`。`bytes` 是文件的大小，`truncated` 是超过了界面读取的上限。 */
export interface AgentMemory {
  content: string;
  bytes: number;
  truncated: boolean;
  modifiedAt: string;
}

/** 页面经 `window.crew` 拿到的全部东西：运行期信息，加上读 Agent 记忆。 */
export interface RendererBridge extends RendererRuntime {
  readMemory(agentId: string): Promise<AgentMemory | null>;
}
