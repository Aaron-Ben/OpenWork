CREATE TABLE "user_read_cursors" (
	"user_id" uuid NOT NULL,
	"room_id" uuid NOT NULL,
	"last_read_seq" bigint DEFAULT 0 NOT NULL,
	CONSTRAINT "user_read_cursors_user_id_room_id_pk" PRIMARY KEY("user_id","room_id"),
	CONSTRAINT "user_read_cursors_seq_non_negative" CHECK ("user_read_cursors"."last_read_seq" >= 0)
);
--> statement-breakpoint
ALTER TABLE "user_read_cursors" ADD CONSTRAINT "user_read_cursors_user_id_users_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "user_read_cursors" ADD CONSTRAINT "user_read_cursors_room_id_rooms_id_fk" FOREIGN KEY ("room_id") REFERENCES "public"."rooms"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
-- 手写：已有房间的历史消息不算未读。
INSERT INTO "user_read_cursors" ("user_id", "room_id", "last_read_seq")
SELECT "room_users"."user_id", "room_users"."room_id", "rooms"."next_seq"
FROM "room_users" INNER JOIN "rooms" ON "rooms"."id" = "room_users"."room_id";
