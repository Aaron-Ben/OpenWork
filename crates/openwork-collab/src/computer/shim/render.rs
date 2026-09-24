//! 把 Server 的命令结果渲染成模型读到的文本（collaboration.md §7.3、§9）。列出消息的输出按 Cumora
//! `cli.ts` 截断正文，截断时注明怎样读完整正文。

use serde::Serialize;

use super::{ShimError, ShimOutput};
use crate::protocol::{AgentCommandResponse, AgentCommandResult, MessageView, MuteView};

/// Characters of one body in `openwork inbox` (collaboration.md §7.3, Cumora `cmdInbox`).
const INBOX_BODY_MAX_CHARS: usize = 240;
/// Characters of one body in `openwork messages` (collaboration.md §7.3, Cumora `cmdMessages`).
const MESSAGES_BODY_MAX_CHARS: usize = 280;
/// Characters of one body in `openwork glance` (collaboration.md §7.3, Cumora `cmdGlance`).
const GLANCE_BODY_MAX_CHARS: usize = 200;
/// Characters of one body in a HELD response (collaboration.md §7.3, Cumora `cmdReply`).
const HELD_BODY_MAX_CHARS: usize = 200;
/// Characters of a quoted original under a message (collaboration.md §7.3, Cumora `cmdInbox`).
const QUOTE_BODY_MAX_CHARS: usize = 180;

/// 有正文被截断时列表末尾的说明。
const CUT_NOTE: &str =
    "Long bodies are cut with …; `openwork messages <room-id> --json` prints them in full.";

/// 命令结果的输出格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Format {
    Text,
    /// `messages --json`：完整正文（Cumora `messages --json`）。
    Json,
}

/// 列表里一条正文的显示方式。
#[derive(Clone, Copy)]
enum Excerpt {
    /// 截到 N 字，换行显示为 ` \n `（Cumora inbox / messages / glance）。
    Lines(usize),
    /// 空白压成一个空格后截到 N 字（Cumora HELD）。
    Collapsed(usize),
}

impl Excerpt {
    /// 返回显示用的正文，以及是否截断过。
    fn apply(self, body: &str) -> (String, bool) {
        match self {
            Self::Lines(limit) => {
                let (shown, cut) = cut(body, limit);
                (shown.replace('\n', " \\n "), cut)
            }
            Self::Collapsed(limit) => cut(
                &body.split_whitespace().collect::<Vec<_>>().join(" "),
                limit,
            ),
        }
    }
}

/// 截到 `limit` 个字符，截断时以 `…` 结尾。
fn cut(text: &str, limit: usize) -> (String, bool) {
    match text.char_indices().nth(limit) {
        Some((end, _)) => (format!("{}…", &text[..end]), true),
        None => (text.to_string(), false),
    }
}

/// 退出码：错误 2，HELD 10（模型应该重新决定后再发），其余 0。
pub(super) fn render(
    response: AgentCommandResponse,
    format: Format,
) -> Result<ShimOutput, ShimError> {
    let (text, exit_code) = match response.result {
        AgentCommandResult::Error { code, message } => (format!("{code}: {message}"), 2),
        AgentCommandResult::Held {
            room_id,
            retry_token,
            messages,
        } => (held(&room_id, &retry_token, &messages), 10),
        AgentCommandResult::Inbox {
            carried_over,
            messages,
        } => {
            let title = if carried_over {
                "Inbox (more unread messages remain)"
            } else {
                "Inbox"
            };
            (
                listing(title, &messages, Excerpt::Lines(INBOX_BODY_MAX_CHARS)),
                0,
            )
        }
        AgentCommandResult::Glance {
            room_id,
            compose_anchor,
            members,
            messages,
        } => {
            let names = members
                .iter()
                .map(|member| format!("{} ({})", member.display_name, member.id))
                .collect::<Vec<_>>()
                .join(", ");
            let title = format!("Room {room_id} at {compose_anchor}\nMembers: {names}");
            let text = if messages.is_empty() {
                format!(
                    "{title}\nNo new messages since you last read this room (latest sequence {compose_anchor})."
                )
            } else {
                listing(&title, &messages, Excerpt::Lines(GLANCE_BODY_MAX_CHARS))
            };
            (text, 0)
        }
        AgentCommandResult::Messages { room_id, messages } => {
            let text = match format {
                Format::Json => pretty(&messages)?,
                Format::Text => listing(
                    &format!("Messages in {room_id}"),
                    &messages,
                    Excerpt::Lines(MESSAGES_BODY_MAX_CHARS),
                ),
            };
            (text, 0)
        }
        other => (summary(other)?, 0),
    };
    Ok(ShimOutput { text, exit_code })
}

/// 不含消息列表、也不是错误的结果。
fn summary(result: AgentCommandResult) -> Result<String, ShimError> {
    match result {
        AgentCommandResult::Rooms { rooms } => pretty(&rooms),
        AgentCommandResult::Members { room_id, members } => {
            Ok(format!("Members in {room_id}\n{}", pretty(&members)?))
        }
        AgentCommandResult::Participants { participants } => pretty(&participants),
        AgentCommandResult::Climates { climates } => pretty(&climates),
        AgentCommandResult::Climate { climate } => pretty(&climate),
        AgentCommandResult::MessagePublished { message } => Ok(format!(
            "Published {} in {} at sequence {}",
            message.id, message.room_id, message.sequence
        )),
        AgentCommandResult::Acknowledged { room_id, up_to_seq } => {
            Ok(format!("Acknowledged {room_id} through {up_to_seq}"))
        }
        AgentCommandResult::DirectMessageSent { room_id, message } => Ok(format!(
            "Published {} in direct room {room_id} at sequence {}",
            message.id, message.sequence
        )),
        AgentCommandResult::Boards { boards } => pretty(&boards),
        AgentCommandResult::Board { board } => pretty(&board),
        AgentCommandResult::Cards { cards } => pretty(&cards),
        AgentCommandResult::Card { card } => pretty(&card),
        AgentCommandResult::Muted {
            participant_id,
            mute,
        } => Ok(muted(&participant_id, &mute)),
        AgentCommandResult::Mutes { mutes } => Ok(mute_list(&mutes)),
        AgentCommandResult::Followed { room_id, was_muted } => Ok(if was_muted {
            format!("Following {room_id} again. New messages will resume normal inbox delivery.")
        } else {
            format!("{room_id} was not muted; normal delivery is already active.")
        }),
        // 错误与消息列表由 `render` 处理，不会到这里。
        listing_or_error => pretty(&listing_or_error),
    }
}

/// 静音回执照 Cumora `cmdMute` 原文（collaboration.md §10.1）。
fn muted(participant_id: &str, mute: &MuteView) -> String {
    let room_id = &mute.room_id;
    let title = mute
        .title
        .as_deref()
        .map(|title| format!(" (\"{title}\")"))
        .unwrap_or_default();
    let expiry = match mute.expires_at.as_deref() {
        Some(time) => format!("until {time}"),
        None => "until you follow it again".to_string(),
    };
    format!(
        "Muted {room_id}{title} {expiry}. New group messages will not wake you or enter your inbox. A direct @{participant_id} mention or a reply quoting your message still gets through. Resume with: openwork follow {room_id}"
    )
}

/// `mute list` 照 Cumora 的列表格式。
fn mute_list(mutes: &[MuteView]) -> String {
    if mutes.is_empty() {
        return "(no muted groups)".to_string();
    }
    mutes
        .iter()
        .map(|mute| {
            let expiry = match mute.expires_at.as_deref() {
                Some(time) => format!("until {time}"),
                None => "until you follow it".to_string(),
            };
            format!(
                "• {}  \"{}\"  — {expiry}",
                mute.room_id,
                mute.title.as_deref().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn pretty(value: &impl Serialize) -> Result<String, ShimError> {
    serde_json::to_string_pretty(value).map_err(ShimError::Json)
}

/// HELD（collaboration.md §9.1，Cumora `cli.ts` 的 HELD 文案）：消息没有发出，列出没看过的消息，
/// 直接重发改过的内容即可，原稿照发才带 token。
fn held(room_id: &str, retry_token: &str, messages: &[MessageView]) -> String {
    let title = format!(
        "HELD — your reply was NOT sent. {} newer message(s) in {room_id} you had not seen:",
        messages.len()
    );
    let mut text = listing(&title, messages, Excerpt::Collapsed(HELD_BODY_MAX_CHARS));
    text.push_str(&format!(
        "\n\nYou have now seen these. Decide again against this state, then simply re-send: `openwork reply {room_id} <revised text>` goes through without any flag. Usually your draft is now wrong (counting: post the next number after the latest; a chain: continue from the latest entry; if a peer already delivered what you were about to say, stand down). Only if your original draft is still correct unchanged, re-send it with `--held-token {retry_token}`."
    ));
    text
}

/// 标题加消息列表。每条带消息 id，模型才能用 `--quote` 引用它；引用的原文在下一行
/// （collaboration.md §9.3）。有正文被截断时最后一行说明怎样读完整正文。
fn listing(title: &str, messages: &[MessageView], excerpt: Excerpt) -> String {
    let mut output = title.to_string();
    if messages.is_empty() {
        output.push_str("\n(no messages)");
    }
    let mut any_cut = false;
    for message in messages {
        let (body, cut_body) = excerpt.apply(&message.body);
        any_cut |= cut_body;
        output.push_str(&format!(
            "\n[{}] #{} {} @ {}: {body}",
            message.id, message.sequence, message.author_id, message.room_id
        ));
        if let Some(quoted) = &message.quoted {
            let (quote, _) = Excerpt::Lines(QUOTE_BODY_MAX_CHARS).apply(&quoted.body);
            output.push_str(&format!(
                "\n    ↩ quoting [{}] {}: {quote}",
                quoted.id, quoted.author_name
            ));
        }
    }
    if any_cut {
        output.push('\n');
        output.push_str(CUT_NOTE);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{Format, render};
    use crate::protocol::{
        AgentCommandResponse, AgentCommandResult, MessageView, MuteView, ParticipantView,
        QuotedMessageView,
    };

    fn message(sequence: i64, body: &str) -> MessageView {
        MessageView {
            id: format!("msg-{sequence}"),
            room_id: "room-1".to_string(),
            sequence,
            author_id: "bo".to_string(),
            body: body.to_string(),
            quoted: None,
        }
    }

    fn text(result: AgentCommandResult) -> String {
        let response = AgentCommandResponse {
            result,
            effects: Vec::new(),
        };
        render(response, Format::Text).unwrap().text
    }

    /// glance 没有新消息时说明原因，而不是只写 `(no messages)`。
    #[test]
    fn empty_glance_says_nothing_new_since_the_last_read() {
        let output = text(AgentCommandResult::Glance {
            room_id: "room-1".to_string(),
            compose_anchor: 3,
            members: vec![ParticipantView {
                id: "bo".to_string(),
                kind: "agent".to_string(),
                display_name: "Bo".to_string(),
            }],
            messages: Vec::new(),
        });

        assert_eq!(
            output,
            "Room room-1 at 3\nMembers: Bo (bo)\nNo new messages since you last read this room (latest sequence 3)."
        );
    }

    /// collaboration.md §9.1：HELD 时说明消息没有发出、列出没看过的消息，并告诉模型直接重发新内容
    /// 即可，只有原话照发才需要 `--held-token`（Cumora `cli.ts` 的 HELD 文案）。
    #[test]
    fn held_replies_say_a_plain_resend_goes_through() {
        let output = render(
            AgentCommandResponse {
                result: AgentCommandResult::Held {
                    room_id: "room-1".to_string(),
                    retry_token: "hold-1".to_string(),
                    messages: vec![message(3, "2")],
                },
                effects: Vec::new(),
            },
            Format::Text,
        )
        .unwrap();

        assert_eq!(output.exit_code, 10);
        assert_eq!(
            output.text,
            "HELD — your reply was NOT sent. 1 newer message(s) in room-1 you had not seen:\n\
             [msg-3] #3 bo @ room-1: 2\n\
             \n\
             You have now seen these. Decide again against this state, then simply re-send: `openwork reply room-1 <revised text>` goes through without any flag. Usually your draft is now wrong (counting: post the next number after the latest; a chain: continue from the latest entry; if a peer already delivered what you were about to say, stand down). Only if your original draft is still correct unchanged, re-send it with `--held-token hold-1`."
        );
    }

    /// messages 与 glance 的每行带消息 id，引用的原文在下一行，换行显示为 ` \n `。
    #[test]
    fn acc_12_message_listings_show_ids_and_quoted_originals() {
        let quoted = MessageView {
            quoted: Some(QuotedMessageView {
                id: "msg-1".to_string(),
                author_id: "local-user".to_string(),
                author_name: "User".to_string(),
                body: "Which index\nshould we add?".to_string(),
            }),
            ..message(2, "Use a partial index.")
        };
        let output = text(AgentCommandResult::Messages {
            room_id: "room-1".to_string(),
            messages: vec![quoted],
        });

        assert_eq!(
            output,
            "Messages in room-1\n[msg-2] #2 bo @ room-1: Use a partial index.\n    ↩ quoting [msg-1] User: Which index \\n should we add?"
        );
    }

    /// collaboration.md §7.3、§16 #21：inbox 240 字、messages 280 字、glance 200 字、HELD 压空白后
    /// 200 字，截断处 `…`，有截断时最后一行说明 `messages --json`；没截断时不加说明。
    #[test]
    fn acc_21_listings_cut_long_bodies_like_cumora() {
        let long = format!("line one\n{}", "x".repeat(400));
        let shown = |limit: usize| format!("line one \\n {}…", "x".repeat(limit - 9));
        let note =
            "Long bodies are cut with …; `openwork messages <room-id> --json` prints them in full.";

        let inbox = text(AgentCommandResult::Inbox {
            carried_over: false,
            messages: vec![message(1, &long)],
        });
        assert_eq!(
            inbox,
            format!("Inbox\n[msg-1] #1 bo @ room-1: {}\n{note}", shown(240))
        );

        let messages = text(AgentCommandResult::Messages {
            room_id: "room-1".to_string(),
            messages: vec![message(1, &long), message(2, "short")],
        });
        assert_eq!(
            messages,
            format!(
                "Messages in room-1\n[msg-1] #1 bo @ room-1: {}\n[msg-2] #2 bo @ room-1: short\n{note}",
                shown(280)
            )
        );

        let glance = text(AgentCommandResult::Glance {
            room_id: "room-1".to_string(),
            compose_anchor: 0,
            members: Vec::new(),
            messages: vec![message(1, &long)],
        });
        assert_eq!(
            glance,
            format!(
                "Room room-1 at 0\nMembers: \n[msg-1] #1 bo @ room-1: {}\n{note}",
                shown(200)
            )
        );

        let held = text(AgentCommandResult::Held {
            room_id: "room-1".to_string(),
            retry_token: "hold-1".to_string(),
            messages: vec![message(1, &long)],
        });
        let collapsed = format!("line one {}…", "x".repeat(191));
        assert!(held.starts_with(&format!(
            "HELD — your reply was NOT sent. 1 newer message(s) in room-1 you had not seen:\n[msg-1] #1 bo @ room-1: {collapsed}\n{note}\n\nYou have now seen these."
        )));

        let untouched = text(AgentCommandResult::Messages {
            room_id: "room-1".to_string(),
            messages: vec![message(1, &"y".repeat(280))],
        });
        assert!(!untouched.contains("--json") && !untouched.contains('…'));
    }

    /// collaboration.md §7.3、§16 #21：`messages --json` 输出完整正文（Cumora `messages --json`）。
    #[test]
    fn acc_21_messages_json_prints_full_bodies() {
        let long = "z".repeat(5000);
        let output = render(
            AgentCommandResponse {
                result: AgentCommandResult::Messages {
                    room_id: "room-1".to_string(),
                    messages: vec![message(1, &long)],
                },
                effects: Vec::new(),
            },
            Format::Json,
        )
        .unwrap();

        let parsed: Vec<MessageView> = serde_json::from_str(&output.text).unwrap();
        assert_eq!(parsed, vec![message(1, &long)]);
    }

    fn rendered(result: AgentCommandResult) -> String {
        render(
            AgentCommandResponse {
                result,
                effects: Vec::new(),
            },
            Format::Text,
        )
        .unwrap()
        .text
    }

    fn muted_view(room_id: &str, title: &str, expires_at: Option<&str>) -> MuteView {
        MuteView {
            room_id: room_id.to_string(),
            title: Some(title.to_string()),
            expires_at: expires_at.map(str::to_string),
        }
    }

    /// collaboration.md §10.1：静音、恢复与列表的回执照 Cumora `cmdMute` / `cmdFollow` 原文。
    #[test]
    fn acc_23_mute_receipts_follow_cumora() {
        assert_eq!(
            rendered(AgentCommandResult::Muted {
                participant_id: "ada".to_string(),
                mute: muted_view("room-1", "Release planning", None),
            }),
            "Muted room-1 (\"Release planning\") until you follow it again. New group messages will not wake you or enter your inbox. A direct @ada mention or a reply quoting your message still gets through. Resume with: openwork follow room-1"
        );
        assert_eq!(
            rendered(AgentCommandResult::Muted {
                participant_id: "ada".to_string(),
                mute: muted_view(
                    "room-1",
                    "Release planning",
                    Some("2026-09-25T20:30:00+08:00")
                ),
            }),
            "Muted room-1 (\"Release planning\") until 2026-09-25T20:30:00+08:00. New group messages will not wake you or enter your inbox. A direct @ada mention or a reply quoting your message still gets through. Resume with: openwork follow room-1"
        );
        assert_eq!(
            rendered(AgentCommandResult::Mutes {
                mutes: vec![
                    muted_view("room-1", "Release planning", None),
                    muted_view("room-2", "Ops", Some("2026-09-25T20:30:00+08:00")),
                ],
            }),
            "• room-1  \"Release planning\"  — until you follow it\n• room-2  \"Ops\"  — until 2026-09-25T20:30:00+08:00"
        );
        assert_eq!(
            rendered(AgentCommandResult::Mutes { mutes: Vec::new() }),
            "(no muted groups)"
        );
        assert_eq!(
            rendered(AgentCommandResult::Followed {
                room_id: "room-1".to_string(),
                was_muted: true,
            }),
            "Following room-1 again. New messages will resume normal inbox delivery."
        );
        assert_eq!(
            rendered(AgentCommandResult::Followed {
                room_id: "room-1".to_string(),
                was_muted: false,
            }),
            "room-1 was not muted; normal delivery is already active."
        );
    }
}
