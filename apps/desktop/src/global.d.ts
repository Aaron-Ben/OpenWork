import type { RendererRuntime } from "../electron/contract";

declare global {
  interface Window {
    /** preload 交来的运行期信息。 */
    crew: RendererRuntime;
  }
}
