import { UserId } from "@crew/protocol";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { ensureLocalUser } from "../src/db";
import { agentReadCursors, agents, messages, rooms, runs, users } from "../src/db/schema";
import { createTestDatabase, type TestDatabase } from "./support/database";

// 迁移与数据库约束的集成测试。约束是数据模型的一部分：它们在数据库层拒绝错误数据，这里逐条确认。

let test: TestDatabase;
let userId: UserId;

beforeAll(async () => {
  test = await createTestDatabase();
  userId = await ensureLocalUser(test.db);
});

afterAll(async () => {
  await test.drop();
});

let handles = 0;

async function newAgent(name = "Alice", handle = `agent-${++handles}`) {
  const [agent] = await test.db
    .insert(agents)
    .values({
      displayName: name,
      handle,
      persona: "代码审查者",
      engineId: "opencode",
      model: "opencode-go/deepseek-v4-pro",
    })
    .returning();
  if (!agent) throw new Error("没有创建 Agent");
  return agent;
}

async function newRoom(directKey: string) {
  const [room] = await test.db.insert(rooms).values({ kind: "direct", directKey }).returning();
  if (!room) throw new Error("没有创建房间");
  return room;
}

/** drizzle 把数据库错误包在 cause 里；取出 PostgreSQL 报告的约束名。 */
async function violatedConstraint(promise: Promise<unknown>): Promise<string | undefined> {
  const error: unknown = await promise.then(
    () => undefined,
    (e: unknown) => e,
  );
  const cause = error instanceof Error ? error.cause : undefined;
  return cause instanceof Error && "constraint" in cause ? String(cause.constraint) : undefined;
}

describe("migrations", () => {
  it("create every table of the data model", async () => {
    const result = await test.pool.query<{ table_name: string }>(
      "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public' ORDER BY table_name",
    );
    expect(result.rows.map((row) => row.table_name)).toEqual([
      "agent_read_cursors",
      "agents",
      "message_mentions",
      "messages",
      "room_agents",
      "room_users",
      "rooms",
      "run_events",
      "run_triggers",
      "runs",
      "user_read_cursors",
      "users",
    ]);
  });
});

describe("ensureLocalUser", () => {
  it("keeps exactly one user and returns the same id every time", async () => {
    expect(await ensureLocalUser(test.db)).toBe(userId);
    expect(await test.db.select().from(users)).toHaveLength(1);
  });
});

describe("constraints", () => {
  it("require exactly one author per message", async () => {
    const agent = await newAgent();
    const room = await newRoom("authors");

    expect(await violatedConstraint(test.db.insert(messages).values({ roomId: room.id, seq: 1, body: "无作者" }))).toBe(
      "messages_one_author",
    );
    expect(
      await violatedConstraint(
        test.db
          .insert(messages)
          .values({ roomId: room.id, seq: 1, body: "两个作者", authorUserId: userId, authorAgentId: agent.id }),
      ),
    ).toBe("messages_one_author");

    await test.db.insert(messages).values({ roomId: room.id, seq: 1, body: "用户发的", authorUserId: userId });
    await test.db.insert(messages).values({ roomId: room.id, seq: 2, body: "Agent 发的", authorAgentId: agent.id });
  });

  it("keep sequence numbers unique within a room", async () => {
    const room = await newRoom("sequence");
    await test.db.insert(messages).values({ roomId: room.id, seq: 1, body: "第一条", authorUserId: userId });
    expect(
      await violatedConstraint(
        test.db.insert(messages).values({ roomId: room.id, seq: 1, body: "撞号", authorUserId: userId }),
      ),
    ).toBe("messages_room_seq_unique");
  });

  it("allow only one direct room per pair", async () => {
    await newRoom("user:alice");
    expect(await violatedConstraint(newRoom("user:alice"))).toBe("rooms_direct_key_unique");
  });

  it("reject a message whose author does not exist", async () => {
    const room = await newRoom("ghost");
    const ghost = UserId.parse("00000000-0000-4000-8000-000000000000");
    expect(
      await violatedConstraint(
        test.db.insert(messages).values({ roomId: room.id, seq: 1, body: "幽灵", authorUserId: ghost }),
      ),
    ).toBe("messages_author_user_id_users_id_fk");
  });

  it("keep handles unique and in the allowed format", async () => {
    await newAgent("Alice", "alice");
    expect(await violatedConstraint(newAgent("Alice 2", "alice"))).toBe("agents_handle_unique");
    expect(await violatedConstraint(newAgent("Bob", "Bob"))).toBe("agents_handle_format");
    expect(await violatedConstraint(newAgent("Bob", "-bob"))).toBe("agents_handle_format");
    expect(await violatedConstraint(newAgent("Bob", "b".repeat(33)))).toBe("agents_handle_format");
  });

  it("require a name for a group room", async () => {
    expect(await violatedConstraint(test.db.insert(rooms).values({ kind: "group" }))).toBe("rooms_group_has_name");
    expect(await violatedConstraint(test.db.insert(rooms).values({ kind: "group", name: " " }))).toBe(
      "rooms_group_has_name",
    );
  });

  it("keep the delivered position at or after the read position", async () => {
    const agent = await newAgent();
    const room = await newRoom("cursor");
    expect(
      await violatedConstraint(
        test.db
          .insert(agentReadCursors)
          .values({ agentId: agent.id, roomId: room.id, lastReadSeq: 2, deliveredSeq: 1 }),
      ),
    ).toBe("agent_read_cursors_delivered_not_behind");
  });

  it("keep at most one running run per agent and require an error for a failed run", async () => {
    const agent = await newAgent();
    await test.db.insert(runs).values({ agentId: agent.id, prompt: "p" });
    expect(await violatedConstraint(test.db.insert(runs).values({ agentId: agent.id, prompt: "p" }))).toBe(
      "runs_one_running_per_agent",
    );
    expect(
      await violatedConstraint(
        test.db.insert(runs).values({ agentId: agent.id, prompt: "p", outcome: "failed", endedAt: new Date() }),
      ),
    ).toBe("runs_failed_has_error");
    expect(
      await violatedConstraint(test.db.insert(runs).values({ agentId: agent.id, prompt: "p", outcome: "succeeded" })),
    ).toBe("runs_ended_unless_running");
  });

  it("reject a blank message body", async () => {
    const room = await newRoom("blank");
    expect(
      await violatedConstraint(
        test.db.insert(messages).values({ roomId: room.id, seq: 1, body: "  ", authorUserId: userId }),
      ),
    ).toBe("messages_body_not_blank");
  });
});
