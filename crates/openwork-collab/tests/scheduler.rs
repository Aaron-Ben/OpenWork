use openwork_collab::scheduler::{IdleAction, WakeAction, WakeState};

#[test]
fn busy_injection_always_preserves_a_pending_rerun() {
    let mut state = WakeState::default();
    assert_eq!(state.on_debounce_elapsed(), WakeAction::Start);
    assert!(state.is_running());

    assert_eq!(state.on_debounce_elapsed(), WakeAction::Inject);
    assert!(state.has_pending_rerun());

    assert_eq!(state.on_idle(), IdleAction::Rerun);
    assert!(state.is_running());
    assert!(!state.has_pending_rerun());

    assert_eq!(state.on_idle(), IdleAction::Stop);
    assert!(!state.is_running());
}

#[test]
fn engine_restart_turns_an_active_run_into_a_reliable_rerun() {
    let mut state = WakeState::default();
    assert_eq!(state.on_debounce_elapsed(), WakeAction::Start);
    assert!(state.on_engine_restart());
    assert_eq!(state.on_idle(), IdleAction::Rerun);
    assert!(state.is_running());
}

#[test]
fn failed_initial_dispatch_is_retried_but_failed_injection_keeps_the_idle_rerun() {
    let mut initial = WakeState::default();
    let start = initial.on_debounce_elapsed();
    assert!(initial.on_dispatch_failed(start));
    assert!(!initial.is_running());

    let mut busy = WakeState::default();
    assert_eq!(busy.on_debounce_elapsed(), WakeAction::Start);
    let inject = busy.on_debounce_elapsed();
    assert!(!busy.on_dispatch_failed(inject));
    assert!(busy.has_pending_rerun());
}
