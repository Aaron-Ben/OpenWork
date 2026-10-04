import { defineConfig } from "vitest/config";

// 真实模型的 e2e 通道：只运行 *.e2e.ts，默认的 vitest run 不会选中它们。
export default defineConfig({
  test: { include: ["test/**/*.e2e.ts"] },
});
