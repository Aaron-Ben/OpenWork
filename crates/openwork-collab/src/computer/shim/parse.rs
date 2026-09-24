//! `openwork` 命令行参数的解析（collaboration.md §7.1、§9）。选项写在正文前后都可以，`--` 之后
//! 一律是正文（Cumora `cli-parse.ts` 的 `parseArgs`）。

use std::io::Read as _;

use super::{HELP, ShimError, render::Format};
use crate::protocol::{AgentCommand, MESSAGE_BODY_MAX_BYTES};

/// `messages … --json` 要完整正文（collaboration.md §7.3，Cumora `messages --json`）；返回去掉该选项
/// 后的参数。其他命令的 `--json` 原样留给解析，按未知选项报错。
pub(super) fn output_format(arguments: Vec<String>) -> (Vec<String>, Format) {
    if arguments.first().map(String::as_str) != Some("messages") {
        return (arguments, Format::Text);
    }
    let (rest, json) = take_switch(&arguments, "--json");
    (rest, if json { Format::Json } else { Format::Text })
}

/// 去掉 `--` 之前出现的开关 `switch`，返回其余参数（原顺序）与它是否出现过。
fn take_switch(arguments: &[String], switch: &str) -> (Vec<String>, bool) {
    let mut rest = Vec::with_capacity(arguments.len());
    let mut found = false;
    let mut literal = false;
    for argument in arguments {
        literal |= argument == "--";
        if !literal && argument == switch {
            found = true;
        } else {
            rest.push(argument.clone());
        }
    }
    (rest, found)
}

/// 把命令行参数解析成发给 Server 的命令；参数不合法时返回给模型看的说明。
pub(super) async fn parse_command(arguments: Vec<String>) -> Result<AgentCommand, ShimError> {
    match arguments.as_slice() {
        [command] if command == "inbox" => Ok(AgentCommand::Inbox),
        [command] if command == "rooms" => Ok(AgentCommand::Rooms),
        [command, room_id] if command == "messages" => Ok(AgentCommand::Messages {
            room_id: room_id.clone(),
            tail: 50,
        }),
        [command, room_id, flag, tail] if command == "messages" && flag == "--tail" => {
            Ok(AgentCommand::Messages {
                room_id: room_id.clone(),
                tail: parse_tail(tail)?,
            })
        }
        [command, room_id] if command == "members" => Ok(AgentCommand::Members {
            room_id: room_id.clone(),
        }),
        [command] if command == "participants" => Ok(AgentCommand::Participants),
        [command, room_id] if command == "glance" => Ok(AgentCommand::Glance {
            room_id: room_id.clone(),
        }),
        [command, room_id] if command == "ack" => Ok(AgentCommand::Ack {
            room_id: room_id.clone(),
        }),
        [command, action] if command == "mute" && action == "list" => Ok(AgentCommand::MuteList),
        [command, room_id, tail @ ..] if command == "mute" => {
            let flags = parse_flags(tail, &["--for", "--until"])?;
            Ok(AgentCommand::Mute {
                room_id: room_id.clone(),
                for_minutes: flags
                    .get("--for")
                    .map(|span| parse_mute_minutes(span))
                    .transpose()?,
                until: flags.get("--until").cloned(),
            })
        }
        [command, room_id] if command == "follow" => Ok(AgentCommand::Follow {
            room_id: room_id.clone(),
        }),
        [board, action] if board == "board" && action == "list" => Ok(AgentCommand::BoardList),
        [board, action, board_id] if board == "board" && action == "show" => {
            Ok(AgentCommand::BoardShow {
                board_id: board_id.clone(),
            })
        }
        [card, action] if card == "card" && action == "list" => {
            Ok(AgentCommand::CardList { board_id: None })
        }
        [card, action, flag, board_id]
            if card == "card" && action == "list" && flag == "--board" =>
        {
            Ok(AgentCommand::CardList {
                board_id: Some(board_id.clone()),
            })
        }
        [card, action, card_id] if card == "card" && action == "show" => {
            Ok(AgentCommand::CardShow {
                card_id: card_id.clone(),
            })
        }
        [card, action, card_id] if card == "card" && action == "claim" => {
            Ok(AgentCommand::CardClaim {
                card_id: card_id.clone(),
            })
        }
        [command, participant_id, tail @ ..] if command == "dm" => {
            Ok(AgentCommand::DirectMessage {
                participant_id: participant_id.clone(),
                body: parse_body(tail).await?,
            })
        }
        [climate, action] if climate == "climate" && action == "show" => {
            Ok(AgentCommand::ClimateShow {
                participant_id: None,
            })
        }
        [climate, action, participant_id] if climate == "climate" && action == "show" => {
            Ok(AgentCommand::ClimateShow {
                participant_id: Some(participant_id.clone()),
            })
        }
        [climate, action, participant_id, tail @ ..]
            if climate == "climate" && action == "note" =>
        {
            let body_start = tail
                .iter()
                .position(|argument| matches!(argument.as_str(), "--stdin" | "--file" | "--"))
                .ok_or_else(|| {
                    ShimError::Arguments(
                        "Climate note requires --stdin, --file <path>, or -- <note>".to_string(),
                    )
                })?;
            let flags = parse_flags(&tail[..body_start], &["--affinity", "--trust"])?;
            Ok(AgentCommand::ClimateNote {
                participant_id: participant_id.clone(),
                affinity: parse_score(required_flag(&flags, "--affinity")?, "affinity")?,
                trust: parse_score(required_flag(&flags, "--trust")?, "trust")?,
                note: parse_body(&tail[body_start..]).await?,
            })
        }
        [command, room_id, tail @ ..] if command == "reply" => {
            let (tail, continuation) = take_switch(tail, "--continue");
            let (options, body_arguments) = extract_options(&tail, &["--held-token", "--quote"])?;
            Ok(AgentCommand::Reply {
                room_id: room_id.clone(),
                body: parse_body(&body_arguments).await?,
                held_token: options.get("--held-token").cloned(),
                quoted_message_id: options.get("--quote").cloned(),
                continuation,
            })
        }
        [card, action, tail @ ..] if card == "card" && action == "create" => {
            let flags = parse_flags(
                tail,
                &[
                    "--board",
                    "--column",
                    "--title",
                    "--description",
                    "--assignee",
                ],
            )?;
            Ok(AgentCommand::CardCreate {
                board_id: required_flag(&flags, "--board")?.to_string(),
                column_id: required_flag(&flags, "--column")?.to_string(),
                title: required_flag(&flags, "--title")?.to_string(),
                description: flags.get("--description").cloned(),
                assignee_id: flags.get("--assignee").cloned(),
            })
        }
        [card, action, card_id, assignee_id] if card == "card" && action == "assign" => {
            Ok(AgentCommand::CardAssign {
                card_id: card_id.clone(),
                assignee_id: assignee_id.clone(),
            })
        }
        [card, action, card_id, tail @ ..] if card == "card" && action == "update" => {
            let CardUpdateArgs { title, description } = card_update_args(tail)?;
            let description = match description {
                DescriptionSource::Keep => None,
                DescriptionSource::Text(text) => Some(text),
                DescriptionSource::Stdin => Some(read_stdin_body()?),
                DescriptionSource::File(path) => Some(read_file_body(&path).await?),
            };
            Ok(AgentCommand::CardUpdate {
                card_id: card_id.clone(),
                title,
                description,
            })
        }
        [card, action, card_id, tail @ ..] if card == "card" && action == "move" => {
            let flags = parse_flags(tail, &["--column", "--before-card"])?;
            Ok(AgentCommand::CardMove {
                card_id: card_id.clone(),
                column_id: required_flag(&flags, "--column")?.to_string(),
                before_card_id: flags.get("--before-card").cloned(),
            })
        }
        _ => Err(ShimError::Arguments(format!("unknown command\n\n{HELP}"))),
    }
}

/// 取出 `--` 之前任意位置成对出现的 `allowed` 选项，返回选项与其余参数（原顺序）。模型常把
/// 选项写在正文之后，所以不要求选项在前（Cumora `cli-parse.ts` 的 `parseArgs`）。
fn extract_options(
    arguments: &[String],
    allowed: &[&str],
) -> Result<(std::collections::BTreeMap<String, String>, Vec<String>), ShimError> {
    let mut options = std::collections::BTreeMap::new();
    let mut rest = Vec::new();
    let mut remaining = arguments.iter();
    while let Some(argument) = remaining.next() {
        if argument == "--" {
            rest.push(argument.clone());
            rest.extend(remaining.cloned());
            break;
        }
        if !allowed.contains(&argument.as_str()) {
            rest.push(argument.clone());
            continue;
        }
        let value = remaining
            .next()
            .filter(|value| !value.is_empty() && value.as_str() != "--")
            .ok_or_else(|| ShimError::Arguments(format!("{argument} requires a value")))?;
        if options.insert(argument.clone(), value.clone()).is_some() {
            return Err(ShimError::Arguments(format!(
                "option {argument} must appear once with a non-empty value"
            )));
        }
    }
    Ok((options, rest))
}

/// `card update` 的描述从哪里来；`Keep` 表示不改描述。
#[derive(Debug, PartialEq, Eq)]
enum DescriptionSource {
    Keep,
    Text(String),
    Stdin,
    File(String),
}

#[derive(Debug, PartialEq, Eq)]
struct CardUpdateArgs {
    title: Option<String>,
    description: DescriptionSource,
}

/// 解析 `card update <card-id>` 之后的选项（collaboration.md §11.2）：`--title` 与描述都可选、至少
/// 给一个，描述只能有一个来源，顺序不限。`--description ""` 表示清空描述，所以它允许空值。
fn card_update_args(arguments: &[String]) -> Result<CardUpdateArgs, ShimError> {
    let mut title = None;
    let mut description = DescriptionSource::Keep;
    let mut remaining = arguments.iter();
    while let Some(option) = remaining.next() {
        let source = match option.as_str() {
            "--title" => {
                let value = option_value(option, remaining.next())?;
                if value.trim().is_empty() {
                    return Err(ShimError::Arguments(format!(
                        "{option} requires a non-empty value"
                    )));
                }
                if title.replace(value).is_some() {
                    return Err(ShimError::Arguments(format!(
                        "option {option} must appear once"
                    )));
                }
                continue;
            }
            "--description" => DescriptionSource::Text(option_value(option, remaining.next())?),
            "--file" => DescriptionSource::File(option_value(option, remaining.next())?),
            "--stdin" => DescriptionSource::Stdin,
            _ => return Err(ShimError::Arguments(format!("unknown option {option}"))),
        };
        if description != DescriptionSource::Keep {
            return Err(ShimError::Arguments(
                "pass only one of --description, --stdin, or --file".to_string(),
            ));
        }
        description = source;
    }
    if title.is_none() && description == DescriptionSource::Keep {
        return Err(ShimError::Arguments(
            "nothing to update — pass --title, --description, --stdin, or --file".to_string(),
        ));
    }
    Ok(CardUpdateArgs { title, description })
}

fn option_value(option: &str, value: Option<&String>) -> Result<String, ShimError> {
    value
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| ShimError::Arguments(format!("{option} requires a value")))
}

/// `--for <N>m|h|d|w` 换成分钟（Cumora `parseMuteUntil`，单位不分大小写）。范围 1 分钟到 90 天由
/// Server 校验；这里只拒绝格式不对和装不进 `u32` 的数。
fn parse_mute_minutes(value: &str) -> Result<u32, ShimError> {
    let invalid =
        || ShimError::Arguments("invalid --for duration (use e.g. 30m, 2h, 1d, or 1w)".to_string());
    let value = value.trim();
    let unit = value.chars().next_back().ok_or_else(invalid)?;
    let amount = &value[..value.len() - unit.len_utf8()];
    if amount.is_empty() || !amount.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    let per_unit: u64 = match unit.to_ascii_lowercase() {
        'm' => 1,
        'h' => 60,
        'd' => 24 * 60,
        'w' => 7 * 24 * 60,
        _ => return Err(invalid()),
    };
    amount
        .parse::<u64>()
        .ok()
        .and_then(|amount| amount.checked_mul(per_unit))
        .and_then(|minutes| u32::try_from(minutes).ok())
        .ok_or_else(|| {
            ShimError::Arguments("--for duration must be between 1 minute and 90 days".to_string())
        })
}

fn parse_tail(value: &str) -> Result<u32, ShimError> {
    value
        .parse::<u32>()
        .ok()
        .filter(|value| (1..=200).contains(value))
        .ok_or_else(|| ShimError::Arguments("--tail must be an integer from 1 to 200".to_string()))
}

fn parse_score(value: &str, name: &str) -> Result<f64, ShimError> {
    value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && (-1.0..=1.0).contains(value))
        .ok_or_else(|| ShimError::Arguments(format!("--{name} must be a number from -1 to 1")))
}

fn parse_flags(
    arguments: &[String],
    allowed: &[&str],
) -> Result<std::collections::BTreeMap<String, String>, ShimError> {
    if !arguments.len().is_multiple_of(2) {
        return Err(ShimError::Arguments(
            "each option requires exactly one value".to_string(),
        ));
    }
    let mut flags = std::collections::BTreeMap::new();
    for pair in arguments.chunks_exact(2) {
        if !allowed.contains(&pair[0].as_str()) {
            return Err(ShimError::Arguments(format!("unknown option {}", pair[0])));
        }
        if pair[1].is_empty() || flags.insert(pair[0].clone(), pair[1].clone()).is_some() {
            return Err(ShimError::Arguments(format!(
                "option {} must appear once with a non-empty value",
                pair[0]
            )));
        }
    }
    Ok(flags)
}

fn required_flag<'a>(
    flags: &'a std::collections::BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, ShimError> {
    flags
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| ShimError::Arguments(format!("missing {name}")))
}

/// 正文可以直接写在 id 之后（多个参数按空格拼接，与 Cumora `reply <convo_id> "<body>"` 相同），
/// 也可以用 `--stdin` / `--file` 避开 shell 引号，或用 `--` 发送以 `--` 开头的文本。
async fn parse_body(arguments: &[String]) -> Result<String, ShimError> {
    let body = match arguments {
        [] => return Err(missing_body()),
        [flag] if flag == "--stdin" => read_stdin_body()?,
        [flag, path] if flag == "--file" => read_file_body(path).await?,
        [separator] if separator == "--" => return Err(missing_body()),
        [separator, words @ ..] if separator == "--" => words.join(" "),
        [first, ..] if first.starts_with("--") => {
            return Err(ShimError::Arguments(format!(
                "unknown option {first}; to send text that starts with --, put -- before it"
            )));
        }
        words => words.join(" "),
    };
    if body.len() > MESSAGE_BODY_MAX_BYTES {
        return Err(ShimError::Arguments(format!(
            "body exceeds {MESSAGE_BODY_MAX_BYTES} bytes"
        )));
    }
    Ok(body)
}

fn missing_body() -> ShimError {
    ShimError::Arguments(
        "missing message body; write it after the id, or use --stdin or --file <path> for text with quotes or $"
            .to_string(),
    )
}

fn read_stdin_body() -> Result<String, ShimError> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take((MESSAGE_BODY_MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MESSAGE_BODY_MAX_BYTES {
        return Err(ShimError::Arguments(format!(
            "body exceeds {MESSAGE_BODY_MAX_BYTES} bytes"
        )));
    }
    String::from_utf8(bytes)
        .map_err(|_| ShimError::Arguments("body must be valid UTF-8".to_string()))
}

async fn read_file_body(path: &str) -> Result<String, ShimError> {
    let home = std::env::var("OPENWORK_AGENT_HOME")
        .map_err(|_| ShimError::Environment("OPENWORK_AGENT_HOME is not set"))?;
    let home = tokio::fs::canonicalize(home).await?;
    let path = tokio::fs::canonicalize(path).await?;
    if !path.starts_with(&home) {
        return Err(ShimError::Arguments(
            "--file must resolve inside OPENWORK_AGENT_HOME".to_string(),
        ));
    }
    let metadata = tokio::fs::metadata(&path).await?;
    if metadata.len() > MESSAGE_BODY_MAX_BYTES as u64 {
        return Err(ShimError::Arguments(format!(
            "body exceeds {MESSAGE_BODY_MAX_BYTES} bytes"
        )));
    }
    let bytes = tokio::fs::read(path).await?;
    String::from_utf8(bytes)
        .map_err(|_| ShimError::Arguments("body must be valid UTF-8".to_string()))
}

#[cfg(test)]
mod tests {
    use super::{
        CardUpdateArgs, DescriptionSource, card_update_args, output_format, parse_command,
        parse_score, parse_tail,
    };
    use crate::{computer::shim::render::Format, protocol::AgentCommand};

    #[tokio::test]
    async fn parses_read_and_climate_commands() {
        assert_eq!(
            parse_command(vec!["rooms".to_string()]).await.unwrap(),
            AgentCommand::Rooms
        );
        assert_eq!(
            parse_command(vec![
                "messages".to_string(),
                "room-1".to_string(),
                "--tail".to_string(),
                "25".to_string(),
            ])
            .await
            .unwrap(),
            AgentCommand::Messages {
                room_id: "room-1".to_string(),
                tail: 25,
            }
        );
        assert_eq!(
            parse_command(vec![
                "climate".to_string(),
                "note".to_string(),
                "beta".to_string(),
                "--affinity".to_string(),
                "0.75".to_string(),
                "--trust".to_string(),
                "-0.25".to_string(),
                "--".to_string(),
                "Strong technically; verify estimates.".to_string(),
            ])
            .await
            .unwrap(),
            AgentCommand::ClimateNote {
                participant_id: "beta".to_string(),
                affinity: 0.75,
                trust: -0.25,
                note: "Strong technically; verify estimates.".to_string(),
            }
        );
    }

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn reply(body: &str, held_token: Option<&str>) -> AgentCommand {
        AgentCommand::Reply {
            room_id: "room-1".to_string(),
            body: body.to_string(),
            held_token: held_token.map(str::to_string),
            quoted_message_id: None,
            continuation: false,
        }
    }

    /// collaboration.md §9.4、§16 #20：`--continue` 写在正文前后都可以，`--` 之后是正文。
    #[tokio::test]
    async fn acc_20_reply_takes_continue_anywhere_outside_the_body() {
        let continued = |body: &str| AgentCommand::Reply {
            room_id: "room-1".to_string(),
            body: body.to_string(),
            held_token: None,
            quoted_message_id: None,
            continuation: true,
        };
        assert_eq!(
            parse_command(arguments(&[
                "reply",
                "room-1",
                "--continue",
                "One more thing."
            ]))
            .await
            .unwrap(),
            continued("One more thing.")
        );
        assert_eq!(
            parse_command(arguments(&[
                "reply",
                "room-1",
                "One more thing.",
                "--continue"
            ]))
            .await
            .unwrap(),
            continued("One more thing.")
        );
        assert_eq!(
            parse_command(arguments(&["reply", "room-1", "--", "use", "--continue"]))
                .await
                .unwrap(),
            reply("use --continue", None)
        );
    }

    /// collaboration.md §7.3、§16 #21：只有 `messages` 认 `--json`；其他命令的 `--json` 留给解析报错。
    #[tokio::test]
    async fn acc_21_only_messages_takes_json() {
        let (rest, format) =
            output_format(arguments(&["messages", "room-1", "--json", "--tail", "5"]));
        assert_eq!(format, Format::Json);
        assert_eq!(
            parse_command(rest).await.unwrap(),
            AgentCommand::Messages {
                room_id: "room-1".to_string(),
                tail: 5,
            }
        );
        let (rest, format) = output_format(arguments(&["reply", "room-1", "--json"]));
        assert_eq!(format, Format::Text);
        assert_eq!(
            parse_command(rest).await.unwrap_err().to_string(),
            "invalid arguments: unknown option --json; to send text that starts with --, put -- before it"
        );
    }

    /// collaboration.md §9、§16 #12：`--quote` 与 `--held-token` 写在正文前后都可以（Cumora
    /// `cli-parse.ts`），`--` 之后都是正文；缺少值时拒绝并说明（模型可见文本逐字断言）。
    #[tokio::test]
    async fn acc_12_reply_takes_a_quote_anywhere_outside_the_body() {
        assert_eq!(
            parse_command(arguments(&[
                "reply",
                "room-1",
                "--held-token",
                "hold-1",
                "--quote",
                "msg-7",
                "Agreed."
            ]))
            .await
            .unwrap(),
            AgentCommand::Reply {
                room_id: "room-1".to_string(),
                body: "Agreed.".to_string(),
                held_token: Some("hold-1".to_string()),
                quoted_message_id: Some("msg-7".to_string()),
                continuation: false,
            }
        );
        assert_eq!(
            parse_command(arguments(&[
                "reply",
                "room-1",
                "Agreed.",
                "--quote",
                "msg-7",
                "--held-token",
                "hold-1"
            ]))
            .await
            .unwrap(),
            AgentCommand::Reply {
                room_id: "room-1".to_string(),
                body: "Agreed.".to_string(),
                held_token: Some("hold-1".to_string()),
                quoted_message_id: Some("msg-7".to_string()),
                continuation: false,
            }
        );
        assert_eq!(
            parse_command(arguments(&[
                "reply", "room-1", "--", "Agreed.", "--quote", "msg-7"
            ]))
            .await
            .unwrap(),
            AgentCommand::Reply {
                room_id: "room-1".to_string(),
                body: "Agreed. --quote msg-7".to_string(),
                held_token: None,
                quoted_message_id: None,
                continuation: false,
            }
        );
        assert_eq!(
            parse_command(arguments(&["reply", "room-1", "--quote"]))
                .await
                .unwrap_err()
                .to_string(),
            "invalid arguments: --quote requires a value"
        );
        assert_eq!(
            parse_command(arguments(&[
                "reply", "room-1", "--quote", "msg-7", "Agreed.", "--quote", "msg-8"
            ]))
            .await
            .unwrap_err()
            .to_string(),
            "invalid arguments: option --quote must appear once with a non-empty value"
        );
    }

    /// 正文可以直接跟在房间 id 后面（Cumora `reply <convo_id> "<body>"`）；多个参数按空格拼接，
    /// `--` 仍然可用。
    #[tokio::test]
    async fn reply_and_dm_accept_the_body_as_plain_arguments() {
        assert_eq!(
            parse_command(arguments(&["reply", "room-1", "Ship it today."]))
                .await
                .unwrap(),
            reply("Ship it today.", None)
        );
        assert_eq!(
            parse_command(arguments(&["reply", "room-1", "Ship", "it", "today."]))
                .await
                .unwrap(),
            reply("Ship it today.", None)
        );
        assert_eq!(
            parse_command(arguments(&["reply", "room-1", "--", "--not-a-flag"]))
                .await
                .unwrap(),
            reply("--not-a-flag", None)
        );
        assert_eq!(
            parse_command(arguments(&[
                "reply",
                "room-1",
                "--held-token",
                "hold-1",
                "Still needed."
            ]))
            .await
            .unwrap(),
            reply("Still needed.", Some("hold-1"))
        );
        assert_eq!(
            parse_command(arguments(&["dm", "bo", "Can you review card-7?"]))
                .await
                .unwrap(),
            AgentCommand::DirectMessage {
                participant_id: "bo".to_string(),
                body: "Can you review card-7?".to_string(),
            }
        );
    }

    /// 缺正文或出现未知选项时，拒绝并告诉模型正确写法（模型可见文本逐字断言）。
    #[tokio::test]
    async fn unknown_options_and_missing_bodies_are_rejected_with_the_usage() {
        assert_eq!(
            parse_command(arguments(&["reply", "room-1"]))
                .await
                .unwrap_err()
                .to_string(),
            "invalid arguments: missing message body; write it after the id, or use --stdin or --file <path> for text with quotes or $"
        );
        assert_eq!(
            parse_command(arguments(&["reply", "room-1", "--tail", "5"]))
                .await
                .unwrap_err()
                .to_string(),
            "invalid arguments: unknown option --tail; to send text that starts with --, put -- before it"
        );
    }

    #[tokio::test]
    async fn parses_the_complete_card_surface_without_structure_or_delete_commands() {
        assert_eq!(
            parse_command(vec![
                "board".to_string(),
                "show".to_string(),
                "board-1".to_string(),
            ])
            .await
            .unwrap(),
            AgentCommand::BoardShow {
                board_id: "board-1".to_string(),
            }
        );
        assert_eq!(
            parse_command(vec![
                "card".to_string(),
                "assign".to_string(),
                "card-1".to_string(),
                "alpha".to_string(),
            ])
            .await
            .unwrap(),
            AgentCommand::CardAssign {
                card_id: "card-1".to_string(),
                assignee_id: "alpha".to_string(),
            }
        );
        assert_eq!(
            parse_command(vec![
                "card".to_string(),
                "update".to_string(),
                "card-1".to_string(),
                "--title".to_string(),
                "Revised".to_string(),
            ])
            .await
            .unwrap(),
            AgentCommand::CardUpdate {
                card_id: "card-1".to_string(),
                title: Some("Revised".to_string()),
                description: None,
            }
        );
        for forbidden in [
            vec!["board", "delete", "board-1"],
            vec!["column", "create", "Review"],
            vec!["card", "delete", "card-1"],
        ] {
            assert!(
                parse_command(forbidden.into_iter().map(str::to_string).collect())
                    .await
                    .is_err()
            );
        }
    }

    #[test]
    fn bounds_numeric_arguments() {
        assert_eq!(parse_tail("1").unwrap(), 1);
        assert_eq!(parse_tail("200").unwrap(), 200);
        assert!(parse_tail("0").is_err());
        assert!(parse_tail("201").is_err());
        assert_eq!(parse_score("-1", "trust").unwrap(), -1.0);
        assert_eq!(parse_score("1", "trust").unwrap(), 1.0);
        assert!(parse_score("NaN", "trust").is_err());
        assert!(parse_score("1.1", "trust").is_err());
    }

    fn update_args(arguments: &[&str]) -> Result<CardUpdateArgs, String> {
        card_update_args(
            &arguments
                .iter()
                .map(|argument| argument.to_string())
                .collect::<Vec<_>>(),
        )
        .map_err(|error| error.to_string())
    }

    /// collaboration.md §11.2：`card update` 的标题与描述都可选、至少给一个，描述可以来自
    /// `--description`、`--stdin` 或 `--file`，选项顺序不限。
    #[test]
    fn card_update_takes_either_field_and_one_description_source() {
        let args = |title: Option<&str>, description: DescriptionSource| CardUpdateArgs {
            title: title.map(str::to_string),
            description,
        };
        assert_eq!(
            update_args(&["--description", "Only this"]),
            Ok(args(None, DescriptionSource::Text("Only this".to_string())))
        );
        assert_eq!(
            update_args(&["--title", "Renamed"]),
            Ok(args(Some("Renamed"), DescriptionSource::Keep))
        );
        assert_eq!(
            update_args(&["--stdin", "--title", "Renamed"]),
            Ok(args(Some("Renamed"), DescriptionSource::Stdin))
        );
        assert_eq!(
            update_args(&["--file", "notes.md"]),
            Ok(args(None, DescriptionSource::File("notes.md".to_string())))
        );
        assert_eq!(
            update_args(&["--description", ""]),
            Ok(args(None, DescriptionSource::Text(String::new())))
        );
        assert_eq!(
            update_args(&[]),
            Err("invalid arguments: nothing to update — pass --title, --description, --stdin, or --file".to_string())
        );
        assert_eq!(
            update_args(&["--description", "x", "--stdin"]),
            Err(
                "invalid arguments: pass only one of --description, --stdin, or --file".to_string()
            )
        );
        assert_eq!(
            update_args(&["--title"]),
            Err("invalid arguments: --title requires a value".to_string())
        );
        assert_eq!(
            update_args(&["--title", "A", "--title", "B"]),
            Err("invalid arguments: option --title must appear once".to_string())
        );
        assert_eq!(
            update_args(&["--bogus", "x"]),
            Err("invalid arguments: unknown option --bogus".to_string())
        );
    }

    async fn parsed(arguments: &[&str]) -> Result<AgentCommand, String> {
        parse_command(
            arguments
                .iter()
                .map(|argument| argument.to_string())
                .collect(),
        )
        .await
        .map_err(|error| error.to_string())
    }

    /// collaboration.md §10.1：`mute` / `follow` / `mute list` 的写法与期限单位照 Cumora `parseMuteUntil`。
    #[tokio::test]
    async fn acc_23_mute_follow_and_list_parse_like_cumora() {
        let mute = |for_minutes: Option<u32>, until: Option<&str>| AgentCommand::Mute {
            room_id: "room-1".to_string(),
            for_minutes,
            until: until.map(str::to_string),
        };
        assert_eq!(parsed(&["mute", "room-1"]).await, Ok(mute(None, None)));
        for (span, minutes) in [("30m", 30), ("2h", 120), ("1d", 1440), ("1W", 10080)] {
            assert_eq!(
                parsed(&["mute", "room-1", "--for", span]).await,
                Ok(mute(Some(minutes), None))
            );
        }
        assert_eq!(
            parsed(&["mute", "room-1", "--until", "2026-10-01T09:00:00+08:00"]).await,
            Ok(mute(None, Some("2026-10-01T09:00:00+08:00")))
        );
        assert_eq!(
            parsed(&[
                "mute",
                "room-1",
                "--for",
                "2h",
                "--until",
                "2026-10-01T09:00:00+08:00"
            ])
            .await,
            Ok(mute(Some(120), Some("2026-10-01T09:00:00+08:00")))
        );
        assert_eq!(parsed(&["mute", "list"]).await, Ok(AgentCommand::MuteList));
        assert_eq!(
            parsed(&["follow", "room-1"]).await,
            Ok(AgentCommand::Follow {
                room_id: "room-1".to_string()
            })
        );
        for span in ["5x", "2", "h", "-1h"] {
            assert_eq!(
                parsed(&["mute", "room-1", "--for", span]).await,
                Err(
                    "invalid arguments: invalid --for duration (use e.g. 30m, 2h, 1d, or 1w)"
                        .to_string()
                )
            );
        }
        assert_eq!(
            parsed(&["mute", "room-1", "--for", "99999999999w"]).await,
            Err(
                "invalid arguments: --for duration must be between 1 minute and 90 days"
                    .to_string()
            )
        );
    }
}
