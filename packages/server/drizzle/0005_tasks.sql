CREATE TABLE "tasks" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"room_id" uuid NOT NULL,
	"number" integer NOT NULL,
	"title" text NOT NULL,
	"status" text DEFAULT 'todo' NOT NULL,
	"assignee_agent_id" uuid,
	"created_by_user_id" uuid,
	"created_by_agent_id" uuid,
	"message_id" uuid NOT NULL,
	"claimed_at" timestamp with time zone,
	"completed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "tasks_message_id_unique" UNIQUE("message_id"),
	CONSTRAINT "tasks_room_number_unique" UNIQUE("room_id","number"),
	CONSTRAINT "tasks_number_positive" CHECK ("tasks"."number" > 0),
	CONSTRAINT "tasks_title_not_blank" CHECK (btrim("tasks"."title") <> ''),
	CONSTRAINT "tasks_status_known" CHECK ("tasks"."status" IN ('todo', 'in_progress', 'in_review', 'done', 'closed')),
	CONSTRAINT "tasks_one_creator" CHECK (num_nonnulls("tasks"."created_by_user_id", "tasks"."created_by_agent_id") = 1),
	CONSTRAINT "tasks_working_has_assignee" CHECK ("tasks"."status" NOT IN ('in_progress', 'in_review') OR "tasks"."assignee_agent_id" IS NOT NULL)
);
--> statement-breakpoint
ALTER TABLE "messages" ADD COLUMN "kind" text DEFAULT 'text' NOT NULL;--> statement-breakpoint
ALTER TABLE "tasks" ADD CONSTRAINT "tasks_room_id_rooms_id_fk" FOREIGN KEY ("room_id") REFERENCES "public"."rooms"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "tasks" ADD CONSTRAINT "tasks_assignee_agent_id_agents_id_fk" FOREIGN KEY ("assignee_agent_id") REFERENCES "public"."agents"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "tasks" ADD CONSTRAINT "tasks_created_by_user_id_users_id_fk" FOREIGN KEY ("created_by_user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "tasks" ADD CONSTRAINT "tasks_created_by_agent_id_agents_id_fk" FOREIGN KEY ("created_by_agent_id") REFERENCES "public"."agents"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "tasks" ADD CONSTRAINT "tasks_message_id_messages_id_fk" FOREIGN KEY ("message_id") REFERENCES "public"."messages"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "messages" ADD CONSTRAINT "messages_kind_known" CHECK ("messages"."kind" IN ('text', 'system'));