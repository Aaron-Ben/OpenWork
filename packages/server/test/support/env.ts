import { existsSync } from "node:fs";
import { resolve } from "node:path";

const repoRoot = resolve(import.meta.dirname, "../../../..");

export interface TestEnv {
  /** 有建库权限的 PostgreSQL，测试在上面建临时数据库。 */
  databaseUrl: string;
}

/**
 * 测试用的 PostgreSQL 地址。环境变量没有设置时读取根目录的 `.env`。
 * 缺少时直接报错，不跳过：集成测试与冒烟测试是提交前必须通过的证据。
 */
export function testEnv(): TestEnv {
  const envFile = resolve(repoRoot, ".env");
  if (!process.env.TEST_DATABASE_URL && existsSync(envFile)) {
    process.loadEnvFile(envFile);
  }
  const databaseUrl = process.env.TEST_DATABASE_URL;
  if (!databaseUrl) {
    throw new Error("测试需要 TEST_DATABASE_URL，见 .env.example；并先运行 docker compose up -d --wait");
  }
  return { databaseUrl };
}
