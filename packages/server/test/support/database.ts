import { randomUUID } from "node:crypto";
import { resolve } from "node:path";
import pg from "pg";
import { createDatabase, type Database, migrateDatabase } from "../../src/db";
import { testEnv } from "./env";

// 集成测试连接真实的 PostgreSQL。每个测试文件建一个独立的临时数据库并执行全部迁移，
// 用完删除，测试之间互不影响，也可以并行运行。

const migrationsFolder = resolve(import.meta.dirname, "../../drizzle");

export interface TestDatabase {
  /** 临时数据库的连接地址，供要自己连接数据库的进程使用（例如冒烟测试启动的 Server）。 */
  url: string;
  db: Database;
  pool: pg.Pool;
  /** 关闭连接并删除临时数据库。 */
  drop(): Promise<void>;
}

export async function createTestDatabase(): Promise<TestDatabase> {
  const admin = testEnv().databaseUrl;
  const name = `crew_test_${randomUUID().replaceAll("-", "")}`;
  await withAdmin(admin, (client) => client.query(`CREATE DATABASE ${name}`));

  const url = new URL(admin);
  url.pathname = `/${name}`;
  const pool = new pg.Pool({ connectionString: url.toString() });
  const db = createDatabase(pool);
  await migrateDatabase(db, migrationsFolder);

  return {
    url: url.toString(),
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
