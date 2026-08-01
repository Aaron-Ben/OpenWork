#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Mode {
    Readonly,
    InPlace { backup_suffix: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub(crate) mode: Mode,
    pub(crate) files: Vec<String>,
}

pub(crate) fn analyze(args: &[String]) -> Option<Invocation> {
    let mut expressions = Vec::<&str>::new();
    let mut operands = Vec::<&str>::new();
    let mut in_place = None::<Option<String>>;
    let mut index = 0;
    let mut options_ended = false;

    while index < args.len() {
        let argument = args[index].as_str();
        if !options_ended && argument == "--" {
            options_ended = true;
            index += 1;
            continue;
        }
        if !options_ended && argument == "-n" {
            index += 1;
            continue;
        }
        if !options_ended && argument == "-e" {
            expressions.push(args.get(index + 1)?.as_str());
            index += 2;
            continue;
        }
        if !options_ended && argument == "-i" {
            if in_place.replace(None).is_some() {
                return None;
            }
            index += 1;
            continue;
        }
        if !options_ended
            && let Some(suffix) = argument.strip_prefix("-i")
            && !suffix.is_empty()
        {
            if !safe_backup_suffix(suffix) || in_place.replace(Some(suffix.to_string())).is_some() {
                return None;
            }
            index += 1;
            continue;
        }
        if !options_ended && argument.starts_with('-') && argument != "-" {
            return None;
        }
        operands.push(argument);
        index += 1;
    }

    let files = if expressions.is_empty() {
        let (expression, files) = operands.split_first()?;
        expressions.push(expression);
        files
    } else {
        operands.as_slice()
    };
    if files.iter().any(|path| dynamic_path(path)) {
        return None;
    }

    let scripts_are_in_place_safe = expressions
        .iter()
        .map(|expression| validate_script(expression))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .all(|is_in_place_safe| is_in_place_safe);
    let mode = match in_place {
        Some(backup_suffix) => {
            if !scripts_are_in_place_safe || files.is_empty() || files.contains(&"-") {
                return None;
            }
            Mode::InPlace { backup_suffix }
        }
        None => Mode::Readonly,
    };

    Some(Invocation {
        mode,
        files: files.iter().map(|path| (*path).to_string()).collect(),
    })
}

/// Returns whether the script is safe for in-place editing. A safe print
/// command is accepted for the readonly path but deliberately returns false.
fn validate_script(script: &str) -> Option<bool> {
    let bytes = script.as_bytes();
    let mut index = skip_ascii_whitespace(bytes, 0);
    let mut in_place_safe = true;
    let mut saw_command = false;

    while index < bytes.len() {
        index = parse_optional_address(bytes, index)?;
        let command = *bytes.get(index)?;
        index += 1;
        match command {
            b's' => index = parse_substitution(bytes, index)?,
            b'd' => {}
            b'p' => in_place_safe = false,
            _ => return None,
        }
        saw_command = true;
        index = skip_ascii_whitespace(bytes, index);
        if index == bytes.len() {
            break;
        }
        if bytes[index] != b';' {
            return None;
        }
        index = skip_ascii_whitespace(bytes, index + 1);
    }

    saw_command.then_some(in_place_safe)
}

fn parse_optional_address(bytes: &[u8], index: usize) -> Option<usize> {
    let Some(mut index) = parse_address(bytes, index) else {
        return Some(index);
    };
    if bytes.get(index).is_some_and(|byte| *byte == b',') {
        index = parse_address(bytes, index + 1)?;
    }
    if bytes.get(index).is_some_and(|byte| *byte == b'!') {
        index += 1;
    }
    Some(index)
}

fn parse_address(bytes: &[u8], index: usize) -> Option<usize> {
    match bytes.get(index).copied()? {
        b'0'..=b'9' => {
            let mut index = index + 1;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
            Some(index)
        }
        b'$' => Some(index + 1),
        b'/' => scan_delimited(bytes, index + 1, b'/'),
        _ => None,
    }
}

fn parse_substitution(bytes: &[u8], index: usize) -> Option<usize> {
    let delimiter = *bytes.get(index)?;
    if delimiter == b'\\'
        || delimiter == b';'
        || delimiter.is_ascii_alphanumeric()
        || delimiter.is_ascii_whitespace()
    {
        return None;
    }
    let pattern_end = scan_delimited(bytes, index + 1, delimiter)?;
    let replacement_end = scan_delimited(bytes, pattern_end, delimiter)?;
    let flags_end = bytes[replacement_end..]
        .iter()
        .position(|byte| *byte == b';')
        .map(|offset| replacement_end + offset)
        .unwrap_or(bytes.len());
    let flags = trim_ascii_whitespace(&bytes[replacement_end..flags_end]);
    if !flags.iter().all(|byte| {
        byte.is_ascii_digit() || matches!(*byte, b'g' | b'p' | b'i' | b'I' | b'm' | b'M')
    }) {
        return None;
    }
    Some(flags_end)
}

fn scan_delimited(bytes: &[u8], mut index: usize, delimiter: u8) -> Option<usize> {
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            byte if byte == delimiter => return Some(index + 1),
            _ => index += 1,
        }
    }
    None
}

fn skip_ascii_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    index
}

fn trim_ascii_whitespace(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn safe_backup_suffix(suffix: &str) -> bool {
    !suffix.contains(['/', '\\', '*', '?', '[', ']', '$', '`']) && !suffix.starts_with('~')
}

fn dynamic_path(path: &str) -> bool {
    path.starts_with('~') || path.contains(['$', '`'])
}

#[cfg(test)]
mod tests {
    use super::{Mode, analyze, validate_script};

    fn args(arguments: &[&str]) -> Vec<String> {
        arguments
            .iter()
            .map(|argument| (*argument).to_string())
            .collect()
    }

    #[test]
    fn delimiter_and_action_table_is_fail_closed() {
        let cases = [
            ("s/a/b/", Some(true)),
            ("s|a|b|", Some(true)),
            ("s#a#b#", Some(true)),
            (r"s/a\/b/c/", Some(true)),
            ("1s/a/b/g;2d", Some(true)),
            ("1p", Some(false)),
            ("w /tmp/x", None),
            ("e id", None),
            ("s/a/b/;w /tmp/x", None),
            ("s/a/b/w /tmp/x", None),
        ];

        for (script, expected) in cases {
            assert_eq!(validate_script(script), expected, "script: {script}");
        }
    }

    #[test]
    fn one_parser_classifies_readonly_and_in_place_invocations() {
        let readonly = analyze(&args(&["-n", "1p", "src/a.ts"])).expect("readonly sed");
        assert_eq!(readonly.mode, Mode::Readonly);

        let in_place = analyze(&args(&["-i.bak", "s/a/b/", "src/a.ts"])).expect("in-place sed");
        assert_eq!(
            in_place.mode,
            Mode::InPlace {
                backup_suffix: Some(".bak".to_string())
            }
        );

        assert!(analyze(&args(&["-i", "1p", "src/a.ts"])).is_none());
        assert!(analyze(&args(&["-f", "script.sed", "src/a.ts"])).is_none());
    }
}
