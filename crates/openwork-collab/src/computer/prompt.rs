//! 每轮 Turn 发给 Engine 的增量 prompt（collaboration.md §7.2）。
//!
//! 只渲染动态部分：时间、triage 提示、未读摘要、Climate 与名册。persona 与协作契约由
//! `AGENTS.md` 作为系统提示词提供（§7.1），这里不重复。格式照搬 Cumora BYOA 的
//! `chatDelta` / `snapshotUnread` / `renderInboxDigest`（`computer/daemon.ts`）。

use std::collections::BTreeMap;

use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

use crate::protocol::{ClimateView, MessageView, RoomView, TeamMember};

/// Message lines in one unread digest (collaboration.md §7.2, Cumora `DIGEST_MAX_MESSAGE_LINES`).
const DIGEST_MAX_MESSAGE_LINES: usize = 40;
/// Characters kept from one message body in the digest (collaboration.md §7.2, Cumora `snapshotUnread`).
const MESSAGE_BODY_MAX_CHARS: usize = 600;
/// 模型看到的时间统一为东八区（.claude/rules/database.md §1）。
const CHINA_OFFSET: UtcOffset = match UtcOffset::from_hms(8, 0, 0) {
    Ok(offset) => offset,
    Err(_) => panic!("+08:00 is a valid offset"),
};

/// 渲染一轮消息 Turn 所需的全部输入。
pub(super) struct MessageTurn<'a> {
    pub(super) self_id: &'a str,
    pub(super) now: OffsetDateTime,
    pub(super) triage_note: &'a str,
    pub(super) messages: &'a [MessageView],
    pub(super) rooms: &'a [RoomView],
    pub(super) team: &'a [TeamMember],
    pub(super) climates: &'a [ClimateView],
    pub(super) carried_over: bool,
}

/// 消息 Turn 的增量 prompt。`messages` 按到达顺序排列；房间按第一次出现的顺序分组。
pub(super) fn message_turn_prompt(turn: &MessageTurn<'_>) -> String {
    let mut prompt = time_line(turn.now);
    if !turn.triage_note.trim().is_empty() {
        prompt.push_str(&format!("\n\nTriage focus: {}", turn.triage_note.trim()));
    }
    prompt.push_str(
        "\n\nYour unread messages (already fetched — do not rerun `openwork inbox`; \
         run `openwork glance <room-id>` before posting in a group):\n",
    );
    prompt.push_str(&digest(turn.messages, turn.rooms, turn.team));
    if turn.carried_over {
        prompt.push_str("\nMore unread messages are waiting; they will arrive in a later turn.");
    }
    let climate = climate_section(turn.climates);
    if !climate.is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(&climate);
    }
    push_roster(&mut prompt, turn.self_id, turn.team);
    prompt
}

/// Agenda Turn 的增量 prompt：时间、Server 给出的说明与名册。
pub(super) fn agenda_turn_prompt(
    self_id: &str,
    now: OffsetDateTime,
    brief: &str,
    team: &[TeamMember],
) -> String {
    let mut prompt = time_line(now);
    prompt.push_str(&format!(
        "\n\nHandle this proactive collaboration turn.\n{}",
        brief.trim()
    ));
    push_roster(&mut prompt, self_id, team);
    prompt
}

fn time_line(now: OffsetDateTime) -> String {
    let local = now
        .to_offset(CHINA_OFFSET)
        .replace_nanosecond(0)
        .expect("0 is a valid nanosecond");
    let formatted = local
        .format(&Rfc3339)
        .expect("the current time is within RFC 3339's year range");
    format!("Current time: {formatted}")
}

fn digest(messages: &[MessageView], rooms: &[RoomView], team: &[TeamMember]) -> String {
    let mut order = Vec::<&str>::new();
    let mut by_room = BTreeMap::<&str, Vec<&MessageView>>::new();
    for message in messages {
        let entry = by_room.entry(message.room_id.as_str()).or_default();
        if entry.is_empty() {
            order.push(message.room_id.as_str());
        }
        entry.push(message);
    }
    let counts = order
        .iter()
        .map(|room| by_room[room].len())
        .collect::<Vec<_>>();
    let shown = water_fill(&counts, DIGEST_MAX_MESSAGE_LINES);
    let mut lines = Vec::new();
    for (index, room_id) in order.iter().enumerate() {
        let room_messages = &by_room[room_id];
        lines.push(room_header(room_id, rooms));
        let hidden = room_messages.len() - shown[index];
        if hidden > 0 {
            lines.push(format!(
                "  … {hidden} older unread message(s) not shown — `openwork messages {room_id} --tail {}` to read them",
                room_messages.len()
            ));
        }
        for message in &room_messages[hidden..] {
            lines.push(message_line(message, team));
        }
    }
    lines.join("\n")
}

/// 与 Server inbox 相同的 quietest-first 分配：每个房间最多拿剩余预算的平均份额。
fn water_fill(counts: &[usize], budget: usize) -> Vec<usize> {
    let mut indexes = (0..counts.len()).collect::<Vec<_>>();
    indexes.sort_by_key(|index| (counts[*index], *index));
    let mut shown = vec![0; counts.len()];
    let mut remaining = budget;
    let mut unserved = counts.len();
    for index in indexes {
        shown[index] = counts[index].min(remaining / unserved);
        remaining -= shown[index];
        unserved -= 1;
    }
    shown
}

fn room_header(room_id: &str, rooms: &[RoomView]) -> String {
    let Some(room) = rooms.iter().find(|room| room.id == room_id) else {
        return format!("# {room_id}");
    };
    match room.title.as_deref() {
        Some(title) => format!("# {room_id} [{}] \"{title}\"", room.kind),
        None => format!("# {room_id} [{}]", room.kind),
    }
}

fn message_line(message: &MessageView, team: &[TeamMember]) -> String {
    let body = message
        .body
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let body = body
        .chars()
        .take(MESSAGE_BODY_MAX_CHARS)
        .collect::<String>();
    format!(
        "  [{}] {}: {body}",
        message.id,
        author(&message.author_id, team)
    )
}

fn author(author_id: &str, team: &[TeamMember]) -> String {
    match team.iter().find(|member| member.id == author_id) {
        Some(member) => format!("{} ({})", member.display_name, member.kind),
        None => author_id.to_string(),
    }
}

fn climate_section(climates: &[ClimateView]) -> String {
    if climates.is_empty() {
        return String::new();
    }
    let mut section = "Private Climate (your subjective impressions, not facts):".to_string();
    for climate in climates {
        section.push_str(&format!(
            "\n  about {}: affinity={}, trust={}, note={}",
            climate.about_participant_id,
            climate.affinity,
            climate.trust,
            climate.last_note.as_deref().unwrap_or("none")
        ));
    }
    section
}

fn push_roster(prompt: &mut String, self_id: &str, team: &[TeamMember]) {
    let active = team
        .iter()
        .filter(|member| !member.archived && member.id != self_id)
        .collect::<Vec<_>>();
    if active.is_empty() {
        return;
    }
    prompt.push_str("\n\nYour team (use these ids for @mentions and `openwork dm`):");
    let people = active.iter().filter(|member| member.kind == "user");
    let agents = active.iter().filter(|member| member.kind == "agent");
    let mut people = people.peekable();
    if people.peek().is_some() {
        prompt.push_str("\nPeople — answer them first:");
        for person in people {
            prompt.push_str(&format!("\n- {} — {}", person.id, person.display_name));
        }
    }
    let mut agents = agents.peekable();
    if agents.peek().is_some() {
        prompt.push_str("\nAgents:");
        for agent in agents {
            prompt.push_str(&format!(
                "\n- {} — {}, {}",
                agent.id,
                agent.display_name,
                agent.role.as_deref().unwrap_or("unspecified")
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    const NOW: OffsetDateTime = datetime!(2026-09-24 10:30:00.123 UTC);

    fn member(id: &str, kind: &str, name: &str, role: Option<&str>) -> TeamMember {
        TeamMember {
            id: id.to_string(),
            kind: kind.to_string(),
            display_name: name.to_string(),
            role: role.map(str::to_string),
            archived: false,
        }
    }

    fn team() -> Vec<TeamMember> {
        vec![
            member("local-user", "user", "User", None),
            member("ada", "agent", "Ada", Some("Architect")),
            member("bo", "agent", "Bo", Some("Reviewer")),
            member("cy", "agent", "Cy", None),
        ]
    }

    fn rooms() -> Vec<RoomView> {
        vec![
            RoomView {
                id: "room-g".to_string(),
                kind: "group".to_string(),
                title: Some("Release planning".to_string()),
            },
            RoomView {
                id: "room-d".to_string(),
                kind: "direct".to_string(),
                title: None,
            },
        ]
    }

    fn message(id: &str, room_id: &str, sequence: i64, author_id: &str, body: &str) -> MessageView {
        MessageView {
            id: id.to_string(),
            room_id: room_id.to_string(),
            sequence,
            author_id: author_id.to_string(),
            body: body.to_string(),
        }
    }

    fn turn<'a>(
        messages: &'a [MessageView],
        rooms: &'a [RoomView],
        team: &'a [TeamMember],
        climates: &'a [ClimateView],
    ) -> MessageTurn<'a> {
        MessageTurn {
            self_id: "ada",
            now: NOW,
            triage_note: "",
            messages,
            rooms,
            team,
            climates,
            carried_over: false,
        }
    }

    /// collaboration.md §7.2、§16 #8：时间、房间标题行、显示名与身份、消息 id、Climate 与名册逐字符合；
    /// persona 不在增量里。
    #[test]
    fn acc_08_message_turn_prompt_renders_the_documented_delta() {
        let messages = vec![
            message(
                "msg-1",
                "room-g",
                3,
                "local-user",
                "Reviewer, can you\n  check the migration?",
            ),
            message("msg-2", "room-d", 7, "bo", "Done with card-7."),
            message("msg-3", "room-g", 4, "cy", "I can take the SQL part."),
        ];
        let climates = vec![ClimateView {
            agent_id: "ada".to_string(),
            about_participant_id: "bo".to_string(),
            affinity: 0.4,
            trust: 0.6,
            last_note: Some("careful reviewer".to_string()),
            updated_at: "2026-09-24T10:00:00+08:00".to_string(),
        }];
        let (rooms, team) = (rooms(), team());
        let mut input = turn(&messages, &rooms, &team, &climates);
        input.triage_note = "A human is waiting.";

        assert_eq!(
            message_turn_prompt(&input),
            "Current time: 2026-09-24T18:30:00+08:00\n\
             \n\
             Triage focus: A human is waiting.\n\
             \n\
             Your unread messages (already fetched — do not rerun `openwork inbox`; run `openwork glance <room-id>` before posting in a group):\n\
             # room-g [group] \"Release planning\"\n\
             \x20 [msg-1] User (user): Reviewer, can you check the migration?\n\
             \x20 [msg-3] Cy (agent): I can take the SQL part.\n\
             # room-d [direct]\n\
             \x20 [msg-2] Bo (agent): Done with card-7.\n\
             \n\
             Private Climate (your subjective impressions, not facts):\n\
             \x20 about bo: affinity=0.4, trust=0.6, note=careful reviewer\n\
             \n\
             Your team (use these ids for @mentions and `openwork dm`):\n\
             People — answer them first:\n\
             - local-user — User\n\
             Agents:\n\
             - bo — Bo, Reviewer\n\
             - cy — Cy, unspecified"
        );
    }

    /// collaboration.md §7.2、§16 #8：超过 40 行时按 quietest-first 分配，未显示的条数与读取命令就地写明。
    #[test]
    fn acc_08_digest_over_forty_lines_names_what_it_left_out() {
        let mut messages = (1..=45)
            .map(|sequence| {
                message(
                    &format!("msg-g{sequence}"),
                    "room-g",
                    sequence,
                    "cy",
                    &format!("busy {sequence}"),
                )
            })
            .collect::<Vec<_>>();
        messages.push(message("msg-d1", "room-d", 1, "bo", "quiet"));
        let (rooms, team) = (rooms(), team());
        let prompt = message_turn_prompt(&turn(&messages, &rooms, &team, &[]));

        assert!(prompt.contains(
            "# room-g [group] \"Release planning\"\n  … 6 older unread message(s) not shown — `openwork messages room-g --tail 45` to read them\n  [msg-g7] Cy (agent): busy 7\n"
        ));
        assert!(prompt.contains(
            "  [msg-g45] Cy (agent): busy 45\n# room-d [direct]\n  [msg-d1] Bo (agent): quiet"
        ));
        assert!(!prompt.contains("[msg-g6]"));
        assert_eq!(prompt.matches("\n  [msg-").count(), 40);
    }

    /// collaboration.md §7.2：正文截断到 600 个字符（按字符而非字节）。
    #[test]
    fn long_bodies_are_cut_at_six_hundred_characters() {
        let body = "界".repeat(700);
        let messages = vec![message("msg-1", "room-g", 1, "bo", &body)];
        let (rooms, team) = (rooms(), team());
        let prompt = message_turn_prompt(&turn(&messages, &rooms, &team, &[]));

        assert!(prompt.contains(&format!("  [msg-1] Bo (agent): {}\n", "界".repeat(600))));
        assert!(!prompt.contains(&"界".repeat(601)));
    }

    /// 名册不含自己和已归档的作者；已归档作者的消息仍显示名字；carried_over 时说明还有未读。
    #[test]
    fn roster_skips_self_and_archived_authors_but_their_messages_keep_names() {
        let mut team = team();
        team.push(TeamMember {
            archived: true,
            ..member("dee", "agent", "Dee", Some("Writer"))
        });
        let messages = vec![message("msg-1", "room-g", 1, "dee", "Last words.")];
        let rooms = rooms();
        let mut input = turn(&messages, &rooms, &team, &[]);
        input.carried_over = true;
        let prompt = message_turn_prompt(&input);

        assert!(prompt.contains("  [msg-1] Dee (agent): Last words.\nMore unread messages are waiting; they will arrive in a later turn."));
        assert!(!prompt.contains("- dee"));
        assert!(!prompt.contains("- ada"));
        assert!(!prompt.contains("Triage focus"));
    }

    /// collaboration.md §7.2：Agenda Turn 同样带时间与名册，不重复 persona。
    #[test]
    fn agenda_turn_prompt_carries_time_brief_and_roster() {
        assert_eq!(
            agenda_turn_prompt("ada", NOW, "Follow up on card-9.\n", &team()),
            "Current time: 2026-09-24T18:30:00+08:00\n\
             \n\
             Handle this proactive collaboration turn.\n\
             Follow up on card-9.\n\
             \n\
             Your team (use these ids for @mentions and `openwork dm`):\n\
             People — answer them first:\n\
             - local-user — User\n\
             Agents:\n\
             - bo — Bo, Reviewer\n\
             - cy — Cy, unspecified"
        );
    }
}
