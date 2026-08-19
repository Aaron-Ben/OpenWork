//! Agent id derivation from display names (docs/collaboration.md §3.1).
//!
//! The id is the home-directory name, the OpenCode agent name, and the string
//! teammates use to address each other, so it must be readable, not random.

/// Longest id accepted by the `collab_participants` CHECK constraint.
pub const MAX_AGENT_ID_LEN: usize = 48;

const SHORT_SUFFIX_TRIES: usize = 4;
const LONG_SUFFIX_TRIES: usize = 6;
const SHORT_SUFFIX_LEN: usize = 4;
const LONG_SUFFIX_LEN: usize = 8;

/// Derive the base slug for an Agent id from its display name.
///
/// ASCII letters map to lowercase, spaces/hyphens/underscores become `_`,
/// everything else (including CJK) is dropped. Leading non-letters and
/// separator runs collapse, the result is truncated to [`MAX_AGENT_ID_LEN`].
/// Returns `None` when the name carries no usable ASCII letters.
pub fn derive_agent_slug(name: &str) -> Option<String> {
    let mut slug = String::new();
    for character in name.trim().chars() {
        match character {
            'a'..='z' | '0'..='9' => slug.push(character),
            'A'..='Z' => slug.push(character.to_ascii_lowercase()),
            ' ' | '-' | '_' if !slug.ends_with('_') => slug.push('_'),
            _ => {}
        }
    }
    // Underscore separators survive the char filter above, so strip them from
    // both ends, then drop any digits still leading (ids must start [a-z]).
    let mut base = slug
        .trim_matches('_')
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .trim_matches('_')
        .to_string();
    base.truncate(MAX_AGENT_ID_LEN);
    let base = base.trim_end_matches('_');
    (!base.is_empty()).then(|| base.to_string())
}

/// Acceptance check mirroring the `collab_participants` id CHECK.
pub fn is_valid_agent_id(id: &str) -> bool {
    let mut characters = id.chars();
    matches!(characters.next(), Some('a'..='z'))
        && characters.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
        && id.len() <= MAX_AGENT_ID_LEN
}

/// Derive an unused Agent id from a display name, retrying with a short random
/// suffix when the base (or a truncated base) is taken and degrading to a
/// longer suffix after [`SHORT_SUFFIX_TRIES`] misses.
pub fn derive_agent_id(name: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    let short = (0..SHORT_SUFFIX_TRIES).map(|_| suffix(SHORT_SUFFIX_LEN));
    let long = (0..LONG_SUFFIX_TRIES).map(|_| suffix(LONG_SUFFIX_LEN));
    derive_agent_id_from_suffixes(name, &exists, short, long)
}

fn suffix(len: usize) -> String {
    uuid::Uuid::new_v4().simple().to_string()[..len].to_string()
}

/// Deterministic core of [`derive_agent_id`], parameterised over suffix
/// candidates so collision behaviour is unit-testable without randomness.
pub fn derive_agent_id_from_suffixes(
    name: &str,
    exists: &dyn Fn(&str) -> bool,
    short_suffixes: impl Iterator<Item = String>,
    long_suffixes: impl Iterator<Item = String>,
) -> Option<String> {
    let base = derive_agent_slug(name)?;
    if !exists(&base) {
        return Some(base);
    }
    let suffixed = |base: &str, suffix: String, width: usize| {
        let room = MAX_AGENT_ID_LEN - width - 1;
        format!("{}_{}", &base[..base.len().min(room)], suffix)
    };
    for suffix in short_suffixes {
        let candidate = suffixed(&base, suffix, SHORT_SUFFIX_LEN);
        if !exists(&candidate) {
            return Some(candidate);
        }
    }
    for suffix in long_suffixes {
        let candidate = suffixed(&base, suffix, LONG_SUFFIX_LEN);
        if !exists(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn free(_id: &str) -> bool {
        false
    }

    #[test]
    fn lowercases_the_name() {
        assert_eq!(derive_agent_slug("Alice").as_deref(), Some("alice"));
    }

    #[test]
    fn turns_spaces_and_hyphens_into_underscores() {
        assert_eq!(
            derive_agent_slug("Code Review").as_deref(),
            Some("code_review")
        );
        assert_eq!(
            derive_agent_slug("Code-Review").as_deref(),
            Some("code_review")
        );
    }

    #[test]
    fn collapses_separator_runs_and_trims_the_ends() {
        assert_eq!(
            derive_agent_slug("  Code --  Review  ").as_deref(),
            Some("code_review")
        );
    }

    #[test]
    fn drops_trailing_characters_the_id_cannot_hold() {
        assert_eq!(derive_agent_slug("Alice!!!").as_deref(), Some("alice"));
        assert_eq!(derive_agent_slug("Alice -").as_deref(), Some("alice"));
    }

    #[test]
    fn truncates_to_the_maximum_id_length() {
        let slug = derive_agent_slug(&"A".repeat(120)).expect("ASCII letters derive");
        assert_eq!(slug.len(), MAX_AGENT_ID_LEN);
        assert!(is_valid_agent_id(&slug));
    }

    #[test]
    fn cannot_derive_from_names_without_ascii_letters() {
        assert_eq!(derive_agent_slug("小艾"), None);
        assert_eq!(derive_agent_slug("小 艾!!"), None);
        assert_eq!(derive_agent_slug("42"), None);
        assert_eq!(derive_agent_slug(" "), None);
    }

    #[test]
    fn keeps_the_ascii_part_of_mixed_language_names() {
        assert_eq!(derive_agent_slug("Alice小艾").as_deref(), Some("alice"));
        assert_eq!(
            derive_agent_slug("Code 小 Review").as_deref(),
            Some("code_review")
        );
        assert_eq!(derive_agent_slug("小艾 Alice").as_deref(), Some("alice"));
    }

    #[test]
    fn skips_leading_digits_and_underscores() {
        assert_eq!(derive_agent_slug("42 Alice").as_deref(), Some("alice"));
        assert_eq!(derive_agent_slug("_alice").as_deref(), Some("alice"));
    }

    #[test]
    fn keeps_the_base_when_it_is_unused() {
        let id =
            derive_agent_id_from_suffixes("Alice", &free, std::iter::empty(), std::iter::empty())
                .expect("unused base derives");
        assert_eq!(id, "alice");
    }

    #[test]
    fn appends_a_short_suffix_when_the_base_is_taken() {
        let id = derive_agent_id_from_suffixes(
            "Alice",
            &|id| id == "alice",
            ["7f3a".to_string()].into_iter(),
            std::iter::empty(),
        )
        .expect("a free short suffix derives");
        assert_eq!(id, "alice_7f3a");
        assert!(is_valid_agent_id(&id));
    }

    #[test]
    fn degrades_to_a_longer_suffix_after_short_misses() {
        let id = derive_agent_id_from_suffixes(
            "Alice",
            &|id| id == "alice" || id == "alice_7f3a",
            ["7f3a".to_string()].into_iter(),
            ["2b81c9de".to_string()].into_iter(),
        )
        .expect("a free long suffix derives");
        assert_eq!(id, "alice_2b81c9de");
    }

    #[test]
    fn gives_up_when_every_candidate_is_taken() {
        let id = derive_agent_id_from_suffixes(
            "Alice",
            &|_| true,
            ["7f3a".to_string()].into_iter(),
            ["2b81c9de".to_string()].into_iter(),
        );
        assert_eq!(id, None);
    }

    #[test]
    fn a_truncated_base_that_collides_still_gets_a_suffix() {
        let long_name = format!("{}{}", "A".repeat(MAX_AGENT_ID_LEN), "ignored tail");
        let base = derive_agent_slug(&long_name).expect("long ASCII name derives");
        assert_eq!(base.len(), MAX_AGENT_ID_LEN);
        let id = derive_agent_id_from_suffixes(
            &long_name,
            &|candidate| candidate == base,
            ["7f3a".to_string()].into_iter(),
            std::iter::empty(),
        )
        .expect("a truncated base with a suffix derives");
        assert!(is_valid_agent_id(&id));
        assert!(id.starts_with(&base[..MAX_AGENT_ID_LEN - SHORT_SUFFIX_LEN - 1]));
        assert!(id.ends_with("_7f3a"));
    }
}
