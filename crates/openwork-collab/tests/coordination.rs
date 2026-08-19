use std::time::{Duration, Instant};

use openwork_collab::coordination::{HeldDecision, HeldTokens, SeenCursors, held_precheck};

#[test]
fn seen_cursor_advances_monotonically_and_expires_fail_open() {
    let start = Instant::now();
    let mut cursors = SeenCursors::new(Duration::from_secs(600));

    cursors.observe("alice", "general", 8, start);
    cursors.observe("alice", "general", 5, start + Duration::from_secs(1));

    assert_eq!(
        cursors.highest("alice", "general", start + Duration::from_secs(599)),
        Some(8)
    );
    assert_eq!(
        cursors.highest("alice", "general", start + Duration::from_secs(601)),
        None
    );
}

#[test]
fn held_precheck_only_blocks_stale_group_replies() {
    assert_eq!(held_precheck(3, 9, Some(8)), HeldDecision::Hold);
    assert_eq!(held_precheck(2, 9, Some(8)), HeldDecision::Allow);
    assert_eq!(held_precheck(3, 9, None), HeldDecision::Allow);
    assert_eq!(held_precheck(3, 9, Some(9)), HeldDecision::Allow);
}

#[test]
fn held_token_confirms_only_the_peer_sequence_it_displayed_and_expires() {
    let start = Instant::now();
    let mut tokens = HeldTokens::new(Duration::from_secs(120));
    let token = tokens.issue("alice", "general", 9, start);

    assert_eq!(
        tokens.confirm(&token, "alice", "general", start + Duration::from_secs(119)),
        Some(9)
    );
    assert_eq!(
        tokens.confirm(&token, "alice", "general", start + Duration::from_secs(121)),
        None
    );
    assert_eq!(
        tokens.confirm(&token, "bob", "general", start + Duration::from_secs(1)),
        None
    );
}
