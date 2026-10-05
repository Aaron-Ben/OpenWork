ALTER TABLE "room_agents" ADD COLUMN "muted_at" timestamp with time zone;--> statement-breakpoint
ALTER TABLE "room_agents" ADD COLUMN "muted_until" timestamp with time zone;--> statement-breakpoint
ALTER TABLE "room_agents" ADD CONSTRAINT "room_agents_muted_until_needs_muted_at" CHECK ("room_agents"."muted_until" IS NULL OR "room_agents"."muted_at" IS NOT NULL);