import { z } from "zod";

// 数据库中的 ID 都是 PostgreSQL 生成的 UUID。branded 类型让编译器区分不同实体的 ID，传错时报错。

export const UserId = z.uuid().brand<"UserId">();
export type UserId = z.infer<typeof UserId>;

export const AgentId = z.uuid().brand<"AgentId">();
export type AgentId = z.infer<typeof AgentId>;

export const RoomId = z.uuid().brand<"RoomId">();
export type RoomId = z.infer<typeof RoomId>;

export const MessageId = z.uuid().brand<"MessageId">();
export type MessageId = z.infer<typeof MessageId>;
