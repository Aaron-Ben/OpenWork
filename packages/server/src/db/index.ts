import type { UserId } from "@crew/protocol";
import { drizzle, type NodePgDatabase } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import type pg from "pg";
import * as schema from "./schema";

export type Database = NodePgDatabase<typeof schema>;

export function createDatabase(pool: pg.Pool): Database {
  return drizzle(pool, { schema });
}

/** 执行 `migrationsFolder` 中还没有执行过的迁移。迁移文件由 drizzle-kit 生成。 */
export async function migrateDatabase(db: Database, migrationsFolder: string): Promise<void> {
  await migrate(db, { migrationsFolder });
}

/** 本机用户的显示名，也出现在 Agent 看到的消息里。 */
export const LOCAL_USER_NAME = "User";

/**
 * 保证 `users` 表恰好有一个本机用户，返回它的 ID。
 *
 * @throws 表中已有多于一个用户。本机只有一个用户，多出的行说明数据被外部改动过。
 */
export async function ensureLocalUser(db: Database): Promise<UserId> {
  const existing = await db.select({ id: schema.users.id }).from(schema.users).limit(2);
  if (existing.length > 1) {
    throw new Error("users 表中有多于一个用户，本机应只有一个");
  }
  if (existing[0]) {
    return existing[0].id;
  }
  const [created] = await db
    .insert(schema.users)
    .values({ displayName: LOCAL_USER_NAME })
    .returning({ id: schema.users.id });
  if (!created) {
    throw new Error("创建本机用户失败");
  }
  return created.id;
}
