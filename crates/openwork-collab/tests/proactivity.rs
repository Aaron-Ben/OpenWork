use std::time::{Duration, Instant};

use openwork_collab::proactivity::{
    AutonomousRateGate, DmProgress, IdleRotation, NudgeClaim, NudgeTracker, ScannerDecision,
    ScannerFingerprints, resolve_dm_progress, scanner_fingerprint, should_probe_agent_dm,
};

#[test]
fn stalled_room_cooldown_is_keyed_only_by_room() {
    let mut tracker = NudgeTracker::default();
    let now = Instant::now();

    assert_eq!(tracker.try_claim("general", now), NudgeClaim::Claimed);
    tracker.finish("general", now, true);

    assert_eq!(
        tracker.try_claim("general", now + Duration::from_secs(44 * 60)),
        NudgeClaim::CoolingDown,
    );
    assert_eq!(
        tracker.try_claim("general", now + Duration::from_secs(45 * 60)),
        NudgeClaim::Claimed,
    );
}

#[test]
fn three_declines_stop_nudges_until_a_new_room_message() {
    let mut tracker = NudgeTracker::default();
    let started = Instant::now();

    for turn in 0..3 {
        let now = started + Duration::from_secs(turn * 45 * 60);
        assert_eq!(tracker.try_claim("general", now), NudgeClaim::Claimed);
        tracker.finish("general", now, false);
    }

    let after_cap = started + Duration::from_secs(3 * 45 * 60);
    assert_eq!(
        tracker.try_claim("general", after_cap),
        NudgeClaim::DeclineCap,
    );
    tracker.observe_room_message("general");
    assert_eq!(tracker.try_claim("general", after_cap), NudgeClaim::Claimed,);
}

#[test]
fn a_successful_nudge_resets_earlier_declines() {
    let mut tracker = NudgeTracker::default();
    let started = Instant::now();
    for turn in 0..2 {
        let now = started + Duration::from_secs(turn * 45 * 60);
        assert_eq!(tracker.try_claim("general", now), NudgeClaim::Claimed);
        tracker.finish("general", now, false);
    }
    let success_at = started + Duration::from_secs(2 * 45 * 60);
    assert_eq!(
        tracker.try_claim("general", success_at),
        NudgeClaim::Claimed
    );
    tracker.finish("general", success_at, true);

    for turn in 3..6 {
        let now = started + Duration::from_secs(turn * 45 * 60);
        assert_eq!(tracker.try_claim("general", now), NudgeClaim::Claimed);
        tracker.finish("general", now, false);
    }
    assert_eq!(
        tracker.try_claim("general", started + Duration::from_secs(6 * 45 * 60)),
        NudgeClaim::DeclineCap,
    );
}

#[test]
fn abandoned_in_flight_nudge_claim_has_a_short_fallback() {
    let mut tracker = NudgeTracker::default();
    let now = Instant::now();
    assert_eq!(tracker.try_claim("general", now), NudgeClaim::Claimed);
    assert_eq!(
        tracker.try_claim("general", now + Duration::from_secs(4 * 60)),
        NudgeClaim::ClaimedByPeer,
    );
    assert_eq!(
        tracker.try_claim("general", now + Duration::from_secs(5 * 60)),
        NudgeClaim::Claimed,
    );
}

#[test]
fn scanner_baselines_once_and_deduplicates_an_unchanged_snapshot() {
    let mut fingerprints = ScannerFingerprints::default();
    let now = Instant::now();

    assert_eq!(
        fingerprints.observe("alice", "general:8", now),
        ScannerDecision::Baseline,
    );
    assert_eq!(
        fingerprints.observe("alice", "general:9", now),
        ScannerDecision::Changed,
    );
    assert_eq!(
        fingerprints.observe("alice", "general:9", now + Duration::from_secs(60)),
        ScannerDecision::Duplicate,
    );
}

#[test]
fn agent_dm_loop_detector_runs_on_each_eighth_message() {
    assert!(!should_probe_agent_dm(0));
    assert!(!should_probe_agent_dm(7));
    assert!(should_probe_agent_dm(8));
    assert!(!should_probe_agent_dm(15));
    assert!(should_probe_agent_dm(16));
}

#[test]
fn agent_dm_progress_probe_stops_a_no_progress_exchange() {
    assert_eq!(resolve_dm_progress(true), DmProgress::Continue);
    assert_eq!(resolve_dm_progress(false), DmProgress::Stop);
}

#[test]
fn idle_selection_rotates_across_available_agents() {
    let mut rotation = IdleRotation::default();
    let candidates = vec!["carol".to_string(), "alice".to_string(), "bob".to_string()];

    assert_eq!(rotation.choose(&candidates).as_deref(), Some("alice"));
    assert_eq!(rotation.choose(&candidates).as_deref(), Some("bob"));
    assert_eq!(rotation.choose(&candidates).as_deref(), Some("carol"));
    assert_eq!(rotation.choose(&candidates).as_deref(), Some("alice"));
}

#[test]
fn autonomous_rate_gate_is_independent_per_agent() {
    let mut gate = AutonomousRateGate::default();
    let now = Instant::now();

    assert!(gate.try_acquire("alice", now));
    assert!(!gate.try_acquire("alice", now + Duration::from_secs(59)));
    assert!(gate.try_acquire("bob", now + Duration::from_secs(59)));
    assert!(gate.try_acquire("alice", now + Duration::from_secs(60)));
}

#[test]
fn scanner_fingerprint_is_order_independent_and_changes_with_room_sequence() {
    let first = vec![("ops".to_string(), 12), ("general".to_string(), 8)];
    let reordered = vec![("general".to_string(), 8), ("ops".to_string(), 12)];
    let changed = vec![("general".to_string(), 9), ("ops".to_string(), 12)];

    assert_eq!(scanner_fingerprint(&first), r#"[["general",8],["ops",12]]"#);
    assert_eq!(scanner_fingerprint(&first), scanner_fingerprint(&reordered));
    assert_ne!(scanner_fingerprint(&first), scanner_fingerprint(&changed));
}
