import { existsSync } from "node:fs";
import { resolve } from "node:path";

const repoRoot = resolve(import.meta.dirname, "../../../..");

export interface TestEnv {
  /** 有建库权限的 PostgreSQL，测试在上面建临时数据库。 */
  databaseUrl: string;
  /** 测试用的 Redis。 */
  redisUrl: string;
}

/**
 * 测试用的 PostgreSQL 与 Redis 地址。环境变量没有设置时读取根目录的 `.env`。
 * 缺少时直接报错，不跳过：集成测试与冒烟测试是提交前必须通过的证据。
 */
export function testEnv(): TestEnv {
  const envFile = resolve(repoRoot, ".env");
  if ((!process.env.TEST_DATABASE_URL || !process.env.TEST_REDIS_URL) && existsSync(envFile)) {
    process.loadEnvFile(envFile);
  }
  const databaseUrl = process.env.TEST_DATABASE_URL;
  const redisUrl = process.env.TEST_REDIS_URL;
  if (!databaseUrl || !redisUrl) {
    throw new Error(
      "测试需要 TEST_DATABASE_URL 与 TEST_REDIS_URL，见 .env.example；并先运行 docker compose up -d --wait",
    );
  }
  return { databaseUrl, redisUrl };
}
