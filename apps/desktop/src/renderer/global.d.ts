import type { RendererRuntime } from "../shared/runtime";

declare global {
  interface Window {
    /** preload 交来的运行期信息。 */
    crew: RendererRuntime;
  }
}
