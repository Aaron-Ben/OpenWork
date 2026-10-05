import type { RendererBridge } from "../electron/contract";

declare global {
  interface Window {
    /** preload 交来的运行期信息与读 Agent 记忆的方法。 */
    crew: RendererBridge;
  }
}
