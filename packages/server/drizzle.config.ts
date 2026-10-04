import { defineConfig } from "drizzle-kit";

// 只用于 `pnpm --filter @crew/server db:generate`：对比 schema 与上次的快照，生成迁移 SQL。
export default defineConfig({
  dialect: "postgresql",
  schema: "./src/db/schema.ts",
  out: "./drizzle",
});
