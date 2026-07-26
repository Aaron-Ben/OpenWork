use super::state::{CompactionStateError, MAX_REMINDER_CHARS};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderSection {
    pub title: String,
    pub lines: Vec<String>,
}

pub(super) fn render_system_reminder(
    sections: &[ReminderSection],
) -> Result<String, CompactionStateError> {
    let mut reminder = String::from("<system_reminder format_version=\"1\">\n");
    if sections.is_empty() {
        reminder.push_str("No additional durable runtime state was recorded at compaction time.\n");
    } else {
        for (index, section) in sections.iter().enumerate() {
            if index > 0 {
                reminder.push('\n');
            }
            reminder.push_str("## ");
            reminder.push_str(&escape_reminder_text(section.title.trim()));
            reminder.push('\n');
            for line in &section.lines {
                reminder.push_str(&escape_reminder_text(line.trim_end()));
                reminder.push('\n');
            }
        }
    }
    reminder.push_str("</system_reminder>");
    validate_system_reminder(&reminder)?;
    Ok(reminder)
}

pub(crate) fn validate_system_reminder(reminder: &str) -> Result<(), CompactionStateError> {
    const OPEN: &str = "<system_reminder format_version=\"1\">";
    const CLOSE: &str = "</system_reminder>";

    if reminder.chars().count() > MAX_REMINDER_CHARS {
        return Err(CompactionStateError::ReminderTooLarge);
    }
    if reminder.trim() != reminder
        || !reminder.starts_with(OPEN)
        || !reminder.ends_with(CLOSE)
        || reminder.matches(OPEN).count() != 1
        || reminder.matches(CLOSE).count() != 1
    {
        return Err(CompactionStateError::InvalidReminder(
            "expected exactly one format_version=1 root with no surrounding content".to_string(),
        ));
    }
    let body = &reminder[OPEN.len()..reminder.len() - CLOSE.len()];
    if body.trim().is_empty() {
        return Err(CompactionStateError::InvalidReminder(
            "root body must not be empty".to_string(),
        ));
    }
    if reminder
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(CompactionStateError::InvalidReminder(
            "contained an unsafe control character".to_string(),
        ));
    }
    Ok(())
}

fn escape_reminder_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            character if character.is_control() => escaped.push('\u{fffd}'),
            character => escaped.push(character),
        }
    }
    escaped
}
