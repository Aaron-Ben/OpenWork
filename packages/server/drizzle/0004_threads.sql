ALTER TABLE "rooms" DROP CONSTRAINT "rooms_kind_known";--> statement-breakpoint
ALTER TABLE "rooms" ADD COLUMN "parent_room_id" uuid;--> statement-breakpoint
ALTER TABLE "rooms" ADD COLUMN "parent_message_id" uuid;--> statement-breakpoint
ALTER TABLE "rooms" ADD CONSTRAINT "rooms_parent_room_id_rooms_id_fk" FOREIGN KEY ("parent_room_id") REFERENCES "public"."rooms"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "rooms" ADD CONSTRAINT "rooms_parent_message_id_messages_id_fk" FOREIGN KEY ("parent_message_id") REFERENCES "public"."messages"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "rooms" ADD CONSTRAINT "rooms_parent_message_id_unique" UNIQUE("parent_message_id");--> statement-breakpoint
ALTER TABLE "rooms" ADD CONSTRAINT "rooms_thread_has_parent" CHECK (("rooms"."kind" = 'thread') = ("rooms"."parent_room_id" IS NOT NULL) AND ("rooms"."kind" = 'thread') = ("rooms"."parent_message_id" IS NOT NULL));--> statement-breakpoint
ALTER TABLE "rooms" ADD CONSTRAINT "rooms_kind_known" CHECK ("rooms"."kind" IN ('direct', 'group', 'thread'));