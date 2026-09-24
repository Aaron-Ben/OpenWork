//! Desktop 房间页读取的形状（collaboration-desktop.md §4.2、§7）。Agent 命令不返回这些类型，
//! 模型看到的输出不变。时间都是带 `+08:00` 的 RFC 3339。

use serde::{Deserialize, Serialize};

use super::MessageView;

/// 房间列表的一行。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoomSummaryView {
    pub id: String,
    pub kind: String,
    pub title: Option<String>,
    /// sequence 大于用户已看到的位置、作者不是用户的 normal 消息数。
    pub unread_count: i64,
    pub last_message: Option<LastMessageView>,
    pub last_message_at: Option<String>,
    pub user_is_member: bool,
    /// 成员 ID；用户排在最前，其余按 ID 排序。
    pub member_ids: Vec<String>,
    pub pinned: bool,
}

/// 最近一条 normal 消息；`body` 最多 80 个字符。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LastMessageView {
    pub author_name: String,
    pub body: String,
}

/// 打开房间时的快照：消息与说明行。成员与其当前状态由房间列表的 `memberIds` 与 Agent 列表得出。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoomSnapshotView {
    pub room_id: String,
    pub messages: Vec<RoomMessageView>,
    pub notes: Vec<RoomNoteView>,
}

/// 带作者信息与时间的消息。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoomMessageView {
    #[serde(flatten)]
    pub message: MessageView,
    pub author_name: String,
    /// `user` 或 `agent`。
    pub author_kind: String,
    pub author_role: Option<String>,
    pub created_at: String,
}

/// 说明行（collaboration-desktop.md §7.3），显示在 `after_sequence` 那条消息之后。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RoomNoteView {
    /// `skipped_names` 判断这条人类消息是给 `target_names` 的，没有参与。
    Routing {
        after_sequence: i64,
        skipped_names: Vec<String>,
        target_names: Vec<String>,
    },
    /// 讨论已满一轮，`speaker_name` 开始第二次发言，其余 Agent 不再被叫醒。
    LapFloor {
        after_sequence: i64,
        speaker_name: String,
    },
    /// Agent 之间已连续 20 条消息，暂停到用户下次发言。
    LoopCap { after_sequence: i64 },
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// collaboration-desktop.md §4.2：消息字段平铺，与 `desktop/src/bridge/collab.ts` 的
    /// `CollabRoomMessage` 同形；说明行以 `kind` 区分。
    #[test]
    fn room_messages_flatten_and_notes_are_kind_tagged() {
        let message = RoomMessageView {
            message: MessageView {
                id: "msg-1".to_string(),
                room_id: "room-1".to_string(),
                sequence: 2,
                author_id: "ada".to_string(),
                body: "On it.".to_string(),
                quoted: None,
            },
            author_name: "Ada".to_string(),
            author_kind: "agent".to_string(),
            author_role: Some("Architect".to_string()),
            created_at: "2026-09-25T10:07:14+08:00".to_string(),
        };
        assert_eq!(
            serde_json::to_value(message).unwrap(),
            json!({
                "id": "msg-1",
                "roomId": "room-1",
                "sequence": 2,
                "authorId": "ada",
                "body": "On it.",
                "quoted": null,
                "authorName": "Ada",
                "authorKind": "agent",
                "authorRole": "Architect",
                "createdAt": "2026-09-25T10:07:14+08:00",
            })
        );
        assert_eq!(
            serde_json::to_value(RoomNoteView::Routing {
                after_sequence: 1,
                skipped_names: vec!["Ada".to_string()],
                target_names: vec!["Bo".to_string()],
            })
            .unwrap(),
            json!({ "kind": "routing", "afterSequence": 1, "skippedNames": ["Ada"], "targetNames": ["Bo"] })
        );
        assert_eq!(
            serde_json::to_value(RoomNoteView::LapFloor {
                after_sequence: 3,
                speaker_name: "Cy".to_string(),
            })
            .unwrap(),
            json!({ "kind": "lap_floor", "afterSequence": 3, "speakerName": "Cy" })
        );
        assert_eq!(
            serde_json::to_value(RoomNoteView::LoopCap { after_sequence: 4 }).unwrap(),
            json!({ "kind": "loop_cap", "afterSequence": 4 })
        );
    }

    #[test]
    fn room_summaries_use_camel_case_fields() {
        let summary = RoomSummaryView {
            id: "room-1".to_string(),
            kind: "group".to_string(),
            title: Some("Release".to_string()),
            unread_count: 3,
            last_message: Some(LastMessageView {
                author_name: "Bo".to_string(),
                body: "Index plan".to_string(),
            }),
            last_message_at: Some("2026-09-25T10:05:00+08:00".to_string()),
            user_is_member: true,
            member_ids: vec!["local-user".to_string(), "bo".to_string()],
            pinned: false,
        };
        assert_eq!(
            serde_json::to_value(summary).unwrap(),
            json!({
                "id": "room-1",
                "kind": "group",
                "title": "Release",
                "unreadCount": 3,
                "lastMessage": { "authorName": "Bo", "body": "Index plan" },
                "lastMessageAt": "2026-09-25T10:05:00+08:00",
                "userIsMember": true,
                "memberIds": ["local-user", "bo"],
                "pinned": false,
            })
        );
    }
}
