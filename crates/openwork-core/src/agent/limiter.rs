use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Default number of sub-agent Turns allowed to run at once, excluding the root.
pub const DEFAULT_MAX_ACTIVE_SUB_AGENT_TURNS: usize = 3;

/// Caps how many sub-agent Turns run concurrently under one root Session.
///
/// The limit counts **active Turns**, not registered sub-agents: an idle agent
/// costs a parked task and nothing else, and it is kept around so a follow-up
/// task can reuse it. What actually costs money is a Turn in flight.
#[derive(Debug)]
pub(super) struct TurnSlots {
    active: AtomicUsize,
    max: usize,
}

impl TurnSlots {
    pub(super) fn new(max: usize) -> Self {
        Self {
            active: AtomicUsize::new(0),
            max,
        }
    }

    pub(super) fn max(&self) -> usize {
        self.max
    }

    pub(super) fn active(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    /// Takes a slot, or returns `None` when the cap is reached.
    ///
    /// Uses compare-exchange rather than "increment, then check, then maybe
    /// decrement": the latter lets two racing callers both observe an
    /// over-limit count and briefly exceed the cap.
    pub(super) fn try_acquire(self: &Arc<Self>) -> Option<TurnSlot> {
        let mut current = self.active.load(Ordering::Acquire);
        loop {
            if current >= self.max {
                return None;
            }
            match self.active.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(TurnSlot {
                        slots: Arc::clone(self),
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }
}

/// A held concurrency slot. Returns itself on drop.
///
/// The slot must be RAII rather than "release on the success path": a sub-agent
/// Turn can end by completing, failing, being cancelled, or by its actor being
/// torn down, and a hand-balanced counter will eventually miss one of those and
/// leak the slot until restart.
#[derive(Debug)]
pub struct TurnSlot {
    slots: Arc<TurnSlots>,
}

impl Drop for TurnSlot {
    fn drop(&mut self) {
        self.slots.active.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hands_out_at_most_max_slots() {
        let slots = Arc::new(TurnSlots::new(2));
        let first = slots.try_acquire().expect("first slot");
        let _second = slots.try_acquire().expect("second slot");
        assert!(slots.try_acquire().is_none(), "third must be refused");
        assert_eq!(slots.active(), 2);

        drop(first);
        assert_eq!(slots.active(), 1);
        assert!(slots.try_acquire().is_some(), "slot must be reusable");
    }

    #[test]
    fn a_zero_cap_refuses_everything() {
        let slots = Arc::new(TurnSlots::new(0));
        assert!(slots.try_acquire().is_none());
    }

    #[test]
    fn concurrent_acquire_never_exceeds_the_cap() {
        let slots = Arc::new(TurnSlots::new(4));
        let observed_max = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..16 {
                let slots = Arc::clone(&slots);
                let observed_max = Arc::clone(&observed_max);
                scope.spawn(move || {
                    for _ in 0..500 {
                        if let Some(slot) = slots.try_acquire() {
                            observed_max.fetch_max(slots.active(), Ordering::AcqRel);
                            drop(slot);
                        }
                    }
                });
            }
        });
        assert!(
            observed_max.load(Ordering::Acquire) <= 4,
            "observed {} concurrent slots, cap is 4",
            observed_max.load(Ordering::Acquire)
        );
        assert_eq!(slots.active(), 0, "every slot must be returned");
    }
}
