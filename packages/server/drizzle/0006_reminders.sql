CREATE TABLE "reminders" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"agent_id" uuid NOT NULL,
	"room_id" uuid NOT NULL,
	"title" text NOT NULL,
	"fire_at" timestamp with time zone NOT NULL,
	"repeat" jsonb,
	"status" text DEFAULT 'scheduled' NOT NULL,
	"fired_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "reminders_title_not_blank" CHECK (btrim("reminders"."title") <> ''),
	CONSTRAINT "reminders_status_known" CHECK ("reminders"."status" IN ('scheduled', 'fired', 'canceled'))
);
--> statement-breakpoint
ALTER TABLE "reminders" ADD CONSTRAINT "reminders_agent_id_agents_id_fk" FOREIGN KEY ("agent_id") REFERENCES "public"."agents"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "reminders" ADD CONSTRAINT "reminders_room_id_rooms_id_fk" FOREIGN KEY ("room_id") REFERENCES "public"."rooms"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "reminders_due" ON "reminders" USING btree ("fire_at") WHERE "reminders"."status" = 'scheduled';