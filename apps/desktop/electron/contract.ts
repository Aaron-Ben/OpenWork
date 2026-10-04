// 主进程、preload 与界面三方共用的约定。界面只引用其中的类型。

/** preload 向主进程索取运行期信息的 IPC 通道。 */
export const RUNTIME_CHANNEL = "crew:runtime";

/** 页面经 `window.crew` 读到的运行期信息：Server 地址与 Desktop 凭证。 */
export interface RendererRuntime {
  serverUrl: string;
  desktopToken: string;
}
