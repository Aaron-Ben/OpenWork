//! 房间说明行（collaboration-desktop.md §7.3）：根据本房间的 triage 记录，解释“为什么有人没回复”。
//! 一段对话是两条人类消息之间的消息；每类说明行在一段对话里最多一条。

use std::collections::{BTreeMap, BTreeSet};

use sqlx::{FromRow, PgConnection};

use super::routing::mentions;
use crate::protocol::RoomNoteView;

/// 生成说明行需要的一条消息。
pub(super) struct NoteMessage<'a> {
    pub(super) sequence: i64,
    pub(super) author_is_user: bool,
    pub(super) author_name: &'a str,
    pub(super) body: &'a str,
}

/// 本房间一条会产生说明行的 triage，按写入先后排列。
#[derive(FromRow)]
pub(super) struct NoteTriage {
    agent_name: String,
    up_to_seq: i64,
    source: String,
    response_mode: Option<String>,
}

/// 读取本房间 `routing`、`lap_floor`、`loop_cap` 三种 triage。
pub(super) async fn triages_in(
    connection: &mut PgConnection,
    room_id: &str,
) -> Result<Vec<NoteTriage>, sqlx::Error> {
    sqlx::query_as::<_, NoteTriage>(
        "SELECT agent.display_name AS agent_name, triage.up_to_seq, triage.source,
                triage.response_mode
         FROM collab_triages triage
         JOIN collab_participants agent ON agent.id = triage.agent_id
         WHERE triage.room_id = $1 AND triage.source IN ('routing', 'lap_floor', 'loop_cap')
         ORDER BY triage.created_at, triage.id",
    )
    .bind(room_id)
    .fetch_all(connection)
    .await
}

/// 按 `after_sequence` 排好的说明行。`agents` 是房间里的 Agent（ID 与显示名），用来找出人类消息点名了谁。
pub(super) fn notes(
    messages: &[NoteMessage<'_>],
    triages: &[NoteTriage],
    agents: &[(String, String)],
) -> Vec<RoomNoteView> {
    let mut notes = routing_notes(messages, triages, agents);
    notes.extend(cap_notes(messages, triages));
    notes.sort_by_key(|note| match note {
        RoomNoteView::Routing { after_sequence, .. } => (*after_sequence, 0),
        RoomNoteView::LapFloor { after_sequence, .. } => (*after_sequence, 1),
        RoomNoteView::LoopCap { after_sequence } => (*after_sequence, 2),
    });
    notes
}

/// `sequence` 所在的那段对话：不晚于它的最近一条人类消息，没有时为 `None`。
fn last_human_at_or_before<'a, 'm>(
    messages: &'a [NoteMessage<'m>],
    sequence: i64,
) -> Option<&'a NoteMessage<'m>> {
    messages
        .iter()
        .rev()
        .find(|message| message.author_is_user && message.sequence <= sequence)
}

/// 一条人类消息点名了别人、有 Agent 判断它不是给自己的（`routing` + `me`）时，在这条消息后说明。
fn routing_notes(
    messages: &[NoteMessage<'_>],
    triages: &[NoteTriage],
    agents: &[(String, String)],
) -> Vec<RoomNoteView> {
    let mut skipped = BTreeMap::<i64, Vec<String>>::new();
    for triage in triages.iter().filter(|triage| {
        triage.source == "routing" && triage.response_mode.as_deref() == Some("me")
    }) {
        let Some(human) = last_human_at_or_before(messages, triage.up_to_seq) else {
            continue;
        };
        let names = skipped.entry(human.sequence).or_default();
        if !names.contains(&triage.agent_name) {
            names.push(triage.agent_name.clone());
        }
    }
    skipped
        .into_iter()
        .filter_map(|(sequence, skipped_names)| {
            let human = messages
                .iter()
                .find(|message| message.sequence == sequence)?;
            let target_names = agents
                .iter()
                .filter(|(id, _)| mentions(human.body, id))
                .map(|(_, name)| name.clone())
                .collect::<Vec<_>>();
            (!target_names.is_empty()).then_some(RoomNoteView::Routing {
                after_sequence: sequence,
                skipped_names,
                target_names,
            })
        })
        .collect()
}

/// 每段对话第一次出现 `lap_floor` 或 `loop_cap` 时，在触发它的消息后说明。
fn cap_notes(messages: &[NoteMessage<'_>], triages: &[NoteTriage]) -> Vec<RoomNoteView> {
    let mut seen = BTreeSet::<(Option<i64>, &str)>::new();
    let mut notes = Vec::new();
    for triage in triages
        .iter()
        .filter(|triage| matches!(triage.source.as_str(), "lap_floor" | "loop_cap"))
    {
        let Some(anchor) = messages
            .iter()
            .rev()
            .find(|message| message.sequence <= triage.up_to_seq)
        else {
            continue;
        };
        let segment =
            last_human_at_or_before(messages, anchor.sequence).map(|human| human.sequence);
        if !seen.insert((segment, triage.source.as_str())) {
            continue;
        }
        notes.push(if triage.source == "lap_floor" {
            RoomNoteView::LapFloor {
                after_sequence: anchor.sequence,
                speaker_name: anchor.author_name.to_string(),
            }
        } else {
            RoomNoteView::LoopCap {
                after_sequence: anchor.sequence,
            }
        });
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(sequence: i64, author: &'static str, body: &'static str) -> NoteMessage<'static> {
        NoteMessage {
            sequence,
            author_is_user: author == "User",
            author_name: author,
            body,
        }
    }

    fn triage(agent: &str, up_to_seq: i64, source: &str, mode: Option<&str>) -> NoteTriage {
        NoteTriage {
            agent_name: agent.to_string(),
            up_to_seq,
            source: source.to_string(),
            response_mode: mode.map(str::to_string),
        }
    }

    fn agents() -> Vec<(String, String)> {
        ["ada", "bo", "cy"]
            .into_iter()
            .map(|id| (id.to_string(), id[..1].to_uppercase() + &id[1..]))
            .collect()
    }

    /// collaboration-desktop.md §7.3 路由：判断“给别人”的 Agent 列在点名消息之后，点名对象来自正文。
    #[test]
    fn acc_11_routing_names_who_stepped_aside_and_who_was_addressed() {
        let messages = [
            message(1, "User", "@bo can you check the index?"),
            message(2, "Bo", "Looking."),
        ];
        let triages = [
            triage("Ada", 1, "routing", Some("me")),
            triage("Cy", 1, "routing", Some("me")),
            triage("Ada", 1, "routing", Some("me")),
            triage("Cy", 2, "routing", Some("each")),
        ];
        assert_eq!(
            notes(&messages, &triages, &agents()),
            vec![RoomNoteView::Routing {
                after_sequence: 1,
                skipped_names: vec!["Ada".to_string(), "Cy".to_string()],
                target_names: vec!["Bo".to_string()],
            }]
        );
    }

    /// 没有点名任何 Agent（例如只有 `@all`）时不出路由说明。
    #[test]
    fn routing_without_an_addressed_agent_has_no_note() {
        let messages = [message(1, "User", "@all status?")];
        let triages = [triage("Ada", 1, "routing", Some("me"))];
        assert!(notes(&messages, &triages, &agents()).is_empty());
    }

    /// collaboration-desktop.md §7.3：一轮上限与硬上限每段对话最多一次，人类发言后开始新的一段。
    #[test]
    fn acc_11_caps_appear_once_per_conversation_segment() {
        let messages = [
            message(1, "User", "Plan the release."),
            message(2, "Ada", "Draft."),
            message(3, "Cy", "Second pass."),
            message(4, "Bo", "Agree."),
            message(5, "User", "Continue."),
            message(6, "Cy", "Again."),
        ];
        let triages = [
            triage("Ada", 3, "lap_floor", None),
            triage("Bo", 4, "lap_floor", None),
            triage("Bo", 4, "loop_cap", None),
            triage("Ada", 4, "loop_cap", None),
            triage("Ada", 6, "lap_floor", None),
        ];
        assert_eq!(
            notes(&messages, &triages, &agents()),
            vec![
                RoomNoteView::LapFloor {
                    after_sequence: 3,
                    speaker_name: "Cy".to_string()
                },
                RoomNoteView::LoopCap { after_sequence: 4 },
                RoomNoteView::LapFloor {
                    after_sequence: 6,
                    speaker_name: "Cy".to_string()
                },
            ]
        );
    }
}
