import { randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import pg from "pg";
import { createDatabase, type Database, migrateDatabase } from "../../src/db";

// 集成测试连接真实的 PostgreSQL。每个测试文件建一个独立的临时数据库并执行全部迁移，
// 用完删除，测试之间互不影响，也可以并行运行。

const repoRoot = resolve(import.meta.dirname, "../../../..");
const migrationsFolder = resolve(import.meta.dirname, "../../drizzle");

/** `TEST_DATABASE_URL` 指向一个有建库权限的 PostgreSQL；没有设置时读取根目录的 `.env`。 */
function adminUrl(): string {
  const envFile = resolve(repoRoot, ".env");
  if (!process.env.TEST_DATABASE_URL && existsSync(envFile)) {
    process.loadEnvFile(envFile);
  }
  const url = process.env.TEST_DATABASE_URL;
  if (!url) {
    throw new Error("集成测试需要 TEST_DATABASE_URL，见 .env.example；并先运行 docker compose up -d --wait");
  }
  return url;
}

export interface TestDatabase {
  db: Database;
  pool: pg.Pool;
  /** 关闭连接并删除临时数据库。 */
  drop(): Promise<void>;
}

export async function createTestDatabase(): Promise<TestDatabase> {
  const admin = adminUrl();
  const name = `crew_test_${randomUUID().replaceAll("-", "")}`;
  await withAdmin(admin, (client) => client.query(`CREATE DATABASE ${name}`));

  const url = new URL(admin);
  url.pathname = `/${name}`;
  const pool = new pg.Pool({ connectionString: url.toString() });
  const db = createDatabase(pool);
  await migrateDatabase(db, migrationsFolder);

  return {
    db,
    pool,
    drop: async () => {
      await pool.end();
      await withAdmin(admin, (client) => client.query(`DROP DATABASE ${name} WITH (FORCE)`));
    },
  };
}

async function withAdmin<T>(url: string, run: (client: pg.Client) => Promise<T>): Promise<T> {
  const client = new pg.Client({ connectionString: url });
  await client.connect();
  try {
    return await run(client);
  } finally {
    await client.end();
  }
}
