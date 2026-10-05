CREATE TABLE "run_events" (
	"run_id" uuid NOT NULL,
	"seq" integer NOT NULL,
	"kind" text NOT NULL,
	"at" timestamp with time zone NOT NULL,
	"data" jsonb NOT NULL,
	CONSTRAINT "run_events_run_id_seq_pk" PRIMARY KEY("run_id","seq")
);
--> statement-breakpoint
CREATE TABLE "run_triggers" (
	"run_id" uuid NOT NULL,
	"room_id" uuid NOT NULL,
	"from_seq" bigint NOT NULL,
	"to_seq" bigint NOT NULL,
	CONSTRAINT "run_triggers_run_id_room_id_pk" PRIMARY KEY("run_id","room_id"),
	CONSTRAINT "run_triggers_range" CHECK ("run_triggers"."from_seq" >= 1 AND "run_triggers"."to_seq" >= "run_triggers"."from_seq")
);
--> statement-breakpoint
CREATE TABLE "runs" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"agent_id" uuid NOT NULL,
	"outcome" text DEFAULT 'running' NOT NULL,
	"error" text,
	"prompt" text NOT NULL,
	"started_at" timestamp with time zone DEFAULT now() NOT NULL,
	"ended_at" timestamp with time zone,
	"input_tokens" bigint DEFAULT 0 NOT NULL,
	"output_tokens" bigint DEFAULT 0 NOT NULL,
	"reasoning_tokens" bigint DEFAULT 0 NOT NULL,
	"cache_read_tokens" bigint DEFAULT 0 NOT NULL,
	"cache_write_tokens" bigint DEFAULT 0 NOT NULL,
	"cost" double precision DEFAULT 0 NOT NULL,
	"steps" integer DEFAULT 0 NOT NULL,
	"replies" integer DEFAULT 0 NOT NULL,
	"holds" integer DEFAULT 0 NOT NULL,
	"last_event_seq" integer DEFAULT 0 NOT NULL,
	CONSTRAINT "runs_outcome_known" CHECK ("runs"."outcome" IN ('running', 'succeeded', 'failed', 'cancelled', 'interrupted')),
	CONSTRAINT "runs_failed_has_error" CHECK ("runs"."outcome" <> 'failed' OR "runs"."error" IS NOT NULL),
	CONSTRAINT "runs_ended_unless_running" CHECK (("runs"."outcome" = 'running') = ("runs"."ended_at" IS NULL))
);
--> statement-breakpoint
ALTER TABLE "messages" ADD COLUMN "run_id" uuid;--> statement-breakpoint
ALTER TABLE "messages" ADD COLUMN "held_before" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "run_events" ADD CONSTRAINT "run_events_run_id_runs_id_fk" FOREIGN KEY ("run_id") REFERENCES "public"."runs"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "run_triggers" ADD CONSTRAINT "run_triggers_run_id_runs_id_fk" FOREIGN KEY ("run_id") REFERENCES "public"."runs"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "run_triggers" ADD CONSTRAINT "run_triggers_room_id_rooms_id_fk" FOREIGN KEY ("room_id") REFERENCES "public"."rooms"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "runs" ADD CONSTRAINT "runs_agent_id_agents_id_fk" FOREIGN KEY ("agent_id") REFERENCES "public"."agents"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "runs_one_running_per_agent" ON "runs" USING btree ("agent_id") WHERE "runs"."outcome" = 'running';--> statement-breakpoint
ALTER TABLE "messages" ADD CONSTRAINT "messages_run_id_runs_id_fk" FOREIGN KEY ("run_id") REFERENCES "public"."runs"("id") ON DELETE set null ON UPDATE no action;