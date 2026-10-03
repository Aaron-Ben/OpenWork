//! 点名路由（collaboration.md §8.2）：人类在群里点名了部分 Agent 时，判断本 Agent 是否被点名，
//! 以及未被点名时交给 triage 模型回答的路由题。
//!
//! 点名对象由代码确定，模型只回答“给被点名的人”还是“给全员”，照搬 Cumora `routing.ts`。
//! Server 不调用模型（collaboration.md §1），所以这道题由每个未被点名的 Agent 各自回答。

/// Characters of one message body given to the routing question (collaboration.md §8.2, Cumora `routing.ts`).
const ROUTING_MESSAGE_MAX_CHARS: usize = 2000;

/// Cumora `routing.ts` 的路由题，改为可以一次判断多条消息。
const ROUTING_INSTRUCTIONS: &str = "You route messages in a team chat where some teammates are AI agents.
Each message below explicitly names one or more agents. Decide whether the messages are aimed at THEM, or at the room.
Answer \"me\" when the named agents are the ones expected to act or reply — a direct request, an assignment, a question put to them.
Answer \"each\" when the whole room is still expected to engage — an open question that merely cites someone, a broadcast, a roll call, a request for several independent opinions. If any message is aimed at the room, answer \"each\".
When you are unsure, answer \"each\". Waking an extra agent costs tokens; failing to wake the right one loses the message.
Respond ONLY with a single JSON object: {\"responseMode\": \"me\"|\"each\"}.";

/// 一条人类消息对本 Agent 的点名情况。
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Addressing {
    /// 本 Agent 必须参与：被点名、私聊、`@all`、没有点名任何人，或点名覆盖了全部 Agent。
    Engage,
    /// 只点名了别的 Agent，由路由题决定本 Agent 是否参与。
    NamesOthers { targets: Vec<String> },
}

/// 判断一条人类消息的点名情况所需的输入。
pub(super) struct HumanMessage<'a> {
    pub(super) body: &'a str,
    pub(super) room_kind: &'a str,
    /// 被引用消息的作者，只在作者是 Agent 时提供。
    pub(super) quoted_agent_id: Option<&'a str>,
    /// 房间里除消息作者外的 active Agent。
    pub(super) candidates: &'a [String],
}

/// 点名对象：精确的 `@<id>`（与 mute 例外相同的边界规则），加上被引用消息的 Agent 作者。
pub(super) fn addressing(message: &HumanMessage<'_>, self_id: &str) -> Addressing {
    if message.room_kind == "direct" || mentions_all(message.body) {
        return Addressing::Engage;
    }
    let mut targets = message
        .candidates
        .iter()
        .filter(|candidate| mentions(message.body, candidate))
        .cloned()
        .collect::<Vec<_>>();
    if let Some(quoted) = message.quoted_agent_id
        && message
            .candidates
            .iter()
            .any(|candidate| candidate == quoted)
        && !targets.iter().any(|target| target == quoted)
    {
        targets.push(quoted.to_string());
    }
    if targets.is_empty()
        || targets.len() >= message.candidates.len()
        || targets.iter().any(|target| target == self_id)
    {
        return Addressing::Engage;
    }
    targets.sort();
    Addressing::NamesOthers { targets }
}

/// 路由题：返回 (instructions, input)。`messages` 里每条都是 (正文, 被点名的 Agent, 房间里的全部候选)。
pub(super) fn routing_request(messages: &[(&str, &[String], &[String])]) -> (String, String) {
    let input = messages
        .iter()
        .map(|(body, targets, candidates)| {
            let others = candidates
                .iter()
                .filter(|candidate| !targets.contains(candidate))
                .cloned()
                .collect::<Vec<_>>();
            format!(
                "Named agents: {}\nOther agents in the room: {}\n\nMessage:\n{}",
                targets.join(", "),
                if others.is_empty() {
                    "(none)".to_string()
                } else {
                    others.join(", ")
                },
                body.chars()
                    .take(ROUTING_MESSAGE_MAX_CHARS)
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    (ROUTING_INSTRUCTIONS.to_string(), input)
}

fn is_id_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
}

/// `@<id>` 前后都不是 `[A-Za-z0-9_-]`，与 `messages.rs` 中 mute 例外的 SQL 正则一致；卡片唤醒的点名
/// 也用它（collaboration.md §11.4）。
pub(super) fn mentions(body: &str, id: &str) -> bool {
    let needle = format!("@{id}");
    body.match_indices(&needle).any(|(start, _)| {
        let before = body[..start].chars().next_back();
        let after = body[start + needle.len()..].chars().next();
        before.is_none_or(|character| !is_id_char(character))
            && after.is_none_or(|character| !is_id_char(character))
    })
}

/// `@all` 不区分大小写，前面不是字母数字、`_` 或 `@`，后面不是字母数字、`_` 或 `-`（Cumora `ALL_MENTION_RE`）。
fn mentions_all(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.match_indices("@all").any(|(start, _)| {
        let before = lower[..start].chars().next_back();
        let after = lower[start + 4..].chars().next();
        before.is_none_or(|character| {
            !(character.is_alphanumeric() || matches!(character, '_' | '@'))
        }) && after.is_none_or(|character| !is_id_char(character))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> Vec<String> {
        vec!["ada".to_string(), "bo".to_string(), "cy".to_string()]
    }

    fn message<'a>(body: &'a str, candidates: &'a [String]) -> HumanMessage<'a> {
        HumanMessage {
            body,
            room_kind: "group",
            quoted_agent_id: None,
            candidates,
        }
    }

    fn names(targets: &[&str]) -> Addressing {
        Addressing::NamesOthers {
            targets: targets.iter().map(|target| target.to_string()).collect(),
        }
    }

    /// collaboration.md §8.2、§15 #9：精确 `@<id>` 与引用都算点名；被点名者、`@all`、私聊、
    /// 没有点名、点名覆盖全员时都直接参与。
    #[test]
    fn acc_09_only_messages_naming_other_agents_are_routed() {
        let all = candidates();
        assert_eq!(
            addressing(&message("@bo check this", &all), "ada"),
            names(&["bo"])
        );
        assert_eq!(
            addressing(&message("@bo check this", &all), "bo"),
            Addressing::Engage
        );
        assert_eq!(
            addressing(&message("@bo and @cy", &all), "ada"),
            names(&["bo", "cy"])
        );
        assert_eq!(
            addressing(&message("@ada @bo @cy", &all), "ada"),
            Addressing::Engage
        );
        assert_eq!(
            addressing(&message("@all, and @bo", &all), "ada"),
            Addressing::Engage
        );
        assert_eq!(
            addressing(&message("@ALL please", &all), "ada"),
            Addressing::Engage
        );
        assert_eq!(
            addressing(&message("anyone?", &all), "ada"),
            Addressing::Engage
        );
        let direct = HumanMessage {
            room_kind: "direct",
            ..message("@bo check this", &all)
        };
        assert_eq!(addressing(&direct, "ada"), Addressing::Engage);
        let quote = HumanMessage {
            quoted_agent_id: Some("cy"),
            ..message("Why?", &all)
        };
        assert_eq!(addressing(&quote, "ada"), names(&["cy"]));
        assert_eq!(addressing(&quote, "cy"), Addressing::Engage);
    }

    /// `@` 前后的边界与 mute 例外相同：邮箱、更长的 id、`@allison` 都不算点名。
    #[test]
    fn mention_boundaries_match_the_mute_exception() {
        let all = candidates();
        assert_eq!(
            addressing(&message("mail bo@bo.dev", &all), "ada"),
            Addressing::Engage
        );
        assert_eq!(
            addressing(&message("@bob here", &all), "ada"),
            Addressing::Engage
        );
        assert_eq!(
            addressing(&message("(@bo) ok", &all), "ada"),
            names(&["bo"])
        );
        assert_eq!(
            addressing(&message("@allison and @bo", &all), "ada"),
            names(&["bo"])
        );
        assert_eq!(
            addressing(&message("@@all @bo", &all), "ada"),
            names(&["bo"])
        );
    }

    /// 路由题逐字照 Cumora `routing.ts`，正文截断到 2000 个字符。
    #[test]
    fn routing_request_lists_named_and_other_agents() {
        let all = candidates();
        let targets = vec!["bo".to_string()];
        let body = format!("@bo {}", "x".repeat(2100));
        let (instructions, input) = routing_request(&[(body.as_str(), &targets, &all)]);

        assert!(instructions.ends_with(
            "Respond ONLY with a single JSON object: {\"responseMode\": \"me\"|\"each\"}."
        ));
        assert_eq!(
            input,
            format!(
                "Named agents: bo\nOther agents in the room: ada, cy\n\nMessage:\n@bo {}",
                "x".repeat(1996)
            )
        );
    }
}
