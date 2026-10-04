CREATE TABLE "message_mentions" (
	"message_id" uuid NOT NULL,
	"agent_id" uuid NOT NULL,
	CONSTRAINT "message_mentions_message_id_agent_id_pk" PRIMARY KEY("message_id","agent_id")
);
--> statement-breakpoint
ALTER TABLE "rooms" DROP CONSTRAINT "rooms_kind_known";--> statement-breakpoint
ALTER TABLE "agent_read_cursors" ADD COLUMN "delivered_seq" bigint DEFAULT 0 NOT NULL;--> statement-breakpoint
-- 手写：已有的已读位置之前的消息都已经交给过 Agent。
UPDATE "agent_read_cursors" SET "delivered_seq" = "last_read_seq";--> statement-breakpoint
ALTER TABLE "agents" ADD COLUMN "handle" text;--> statement-breakpoint
-- 手写：已有的 Agent 按名字生成 handle，名字里没有英文字母或数字时用 ID 前缀。
-- 重名的第二个起改用 agent- 加 ID：加 -2 这类后缀可能撞上另一个名字生成的 handle（Bob、Bob、Bob 2）。
UPDATE "agents" SET "handle" = named."handle"
FROM (
	SELECT "id", CASE WHEN n = 1 THEN base ELSE 'agent-' || left(replace("id"::text, '-', ''), 26) END AS "handle"
	FROM (
		SELECT "id", base, row_number() OVER (PARTITION BY base ORDER BY "created_at", "id") AS n
		FROM (
			SELECT "id", "created_at", coalesce(
				nullif(trim(both '-' from left(trim(both '-' from regexp_replace(lower("display_name"), '[^a-z0-9]+', '-', 'g')), 24)), ''),
				'agent-' || left("id"::text, 8)
			) AS base
			FROM "agents"
		) bases
	) numbered
) named
WHERE "agents"."id" = named."id";--> statement-breakpoint
ALTER TABLE "agents" ALTER COLUMN "handle" SET NOT NULL;--> statement-breakpoint
ALTER TABLE "rooms" ADD COLUMN "name" text;--> statement-breakpoint
ALTER TABLE "message_mentions" ADD CONSTRAINT "message_mentions_message_id_messages_id_fk" FOREIGN KEY ("message_id") REFERENCES "public"."messages"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "message_mentions" ADD CONSTRAINT "message_mentions_agent_id_agents_id_fk" FOREIGN KEY ("agent_id") REFERENCES "public"."agents"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "agents" ADD CONSTRAINT "agents_handle_unique" UNIQUE("handle");--> statement-breakpoint
ALTER TABLE "agent_read_cursors" ADD CONSTRAINT "agent_read_cursors_delivered_not_behind" CHECK ("agent_read_cursors"."delivered_seq" >= "agent_read_cursors"."last_read_seq");--> statement-breakpoint
ALTER TABLE "agents" ADD CONSTRAINT "agents_handle_format" CHECK ("agents"."handle" ~ '^[a-z0-9][a-z0-9-]{0,31}$');--> statement-breakpoint
ALTER TABLE "rooms" ADD CONSTRAINT "rooms_group_has_name" CHECK ("rooms"."kind" <> 'group' OR btrim(coalesce("rooms"."name", '')) <> '');--> statement-breakpoint
ALTER TABLE "rooms" ADD CONSTRAINT "rooms_kind_known" CHECK ("rooms"."kind" IN ('direct', 'group'));