-- 房间置顶（collaboration.md §13.3.4、collaboration-desktop.md §4.5）：用户置顶房间的时间，可空。
ALTER TABLE collab_rooms ADD COLUMN user_pinned_at TIMESTAMP WITHOUT TIME ZONE;
