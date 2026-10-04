CREATE TABLE "agent_read_cursors" (
	"agent_id" uuid NOT NULL,
	"room_id" uuid NOT NULL,
	"last_read_seq" bigint DEFAULT 0 NOT NULL,
	CONSTRAINT "agent_read_cursors_agent_id_room_id_pk" PRIMARY KEY("agent_id","room_id"),
	CONSTRAINT "agent_read_cursors_seq_non_negative" CHECK ("agent_read_cursors"."last_read_seq" >= 0)
);
--> statement-breakpoint
CREATE TABLE "agents" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"display_name" text NOT NULL,
	"persona" text NOT NULL,
	"engine_id" text NOT NULL,
	"model" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "agents_display_name_not_blank" CHECK (btrim("agents"."display_name") <> ''),
	CONSTRAINT "agents_persona_not_blank" CHECK (btrim("agents"."persona") <> '')
);
--> statement-breakpoint
CREATE TABLE "messages" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"room_id" uuid NOT NULL,
	"seq" bigint NOT NULL,
	"author_user_id" uuid,
	"author_agent_id" uuid,
	"body" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "messages_room_seq_unique" UNIQUE("room_id","seq"),
	CONSTRAINT "messages_seq_positive" CHECK ("messages"."seq" > 0),
	CONSTRAINT "messages_one_author" CHECK (num_nonnulls("messages"."author_user_id", "messages"."author_agent_id") = 1),
	CONSTRAINT "messages_body_not_blank" CHECK (btrim("messages"."body") <> '')
);
--> statement-breakpoint
CREATE TABLE "room_agents" (
	"room_id" uuid NOT NULL,
	"agent_id" uuid NOT NULL,
	CONSTRAINT "room_agents_room_id_agent_id_pk" PRIMARY KEY("room_id","agent_id")
);
--> statement-breakpoint
CREATE TABLE "room_users" (
	"room_id" uuid NOT NULL,
	"user_id" uuid NOT NULL,
	CONSTRAINT "room_users_room_id_user_id_pk" PRIMARY KEY("room_id","user_id")
);
--> statement-breakpoint
CREATE TABLE "rooms" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"kind" text NOT NULL,
	"direct_key" text,
	"next_seq" bigint DEFAULT 0 NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "rooms_direct_key_unique" UNIQUE("direct_key"),
	CONSTRAINT "rooms_kind_known" CHECK ("rooms"."kind" IN ('direct')),
	CONSTRAINT "rooms_direct_has_key" CHECK ("rooms"."kind" <> 'direct' OR "rooms"."direct_key" IS NOT NULL)
);
--> statement-breakpoint
CREATE TABLE "users" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"display_name" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
ALTER TABLE "agent_read_cursors" ADD CONSTRAINT "agent_read_cursors_agent_id_agents_id_fk" FOREIGN KEY ("agent_id") REFERENCES "public"."agents"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "agent_read_cursors" ADD CONSTRAINT "agent_read_cursors_room_id_rooms_id_fk" FOREIGN KEY ("room_id") REFERENCES "public"."rooms"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "messages" ADD CONSTRAINT "messages_room_id_rooms_id_fk" FOREIGN KEY ("room_id") REFERENCES "public"."rooms"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "messages" ADD CONSTRAINT "messages_author_user_id_users_id_fk" FOREIGN KEY ("author_user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "messages" ADD CONSTRAINT "messages_author_agent_id_agents_id_fk" FOREIGN KEY ("author_agent_id") REFERENCES "public"."agents"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "room_agents" ADD CONSTRAINT "room_agents_room_id_rooms_id_fk" FOREIGN KEY ("room_id") REFERENCES "public"."rooms"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "room_agents" ADD CONSTRAINT "room_agents_agent_id_agents_id_fk" FOREIGN KEY ("agent_id") REFERENCES "public"."agents"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "room_users" ADD CONSTRAINT "room_users_room_id_rooms_id_fk" FOREIGN KEY ("room_id") REFERENCES "public"."rooms"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "room_users" ADD CONSTRAINT "room_users_user_id_users_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;