use super::ALLOCATIONS;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[path = "../physical_compare_remainder_full/mod.rs"]
#[allow(dead_code)]
mod full;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static START: AtomicUsize = AtomicUsize::new(0);
static MEASURED: AtomicUsize = AtomicUsize::new(0);

fn mark(begin: bool) {
    if begin {
        assert!(!ACTIVE.swap(true, Ordering::Relaxed));
        START.store(ALLOCATIONS.load(Ordering::Relaxed), Ordering::Relaxed);
    } else {
        let end = ALLOCATIONS.load(Ordering::Relaxed);
        assert!(ACTIVE.swap(false, Ordering::Relaxed));
        assert_eq!(end - START.load(Ordering::Relaxed), 0);
        MEASURED.fetch_add(1, Ordering::Relaxed);
    }
}

pub(super) fn compare_remainder_steps() {
    let before = MEASURED.load(Ordering::Relaxed);
    let mut total = 0;
    for (index, count) in full::ALLOCATION_COUNTS.into_iter().enumerate() {
        let measured_before = MEASURED.load(Ordering::Relaxed);
        let actual = full::allocation_section(index as u32, mark);
        assert_eq!(actual, (count, 0));
        assert!(!ACTIVE.load(Ordering::Relaxed));
        let intervals = MEASURED.load(Ordering::Relaxed) - measured_before;
        assert!(intervals >= count as usize);
        total += count;
        eprintln!(
            "physical CMP/CMPA allocation section={index} count={} bad={} intervals={intervals}",
            actual.0, actual.1
        );
    }
    assert!(MEASURED.load(Ordering::Relaxed) - before >= total as usize);
}
