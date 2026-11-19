use super::ALLOCATIONS;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[path = "../physical_cmpi.rs"]
#[allow(dead_code)]
mod test_suite;

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
    for (index, count) in test_suite::full::ALLOCATION_COUNTS.into_iter().enumerate() {
        let actual = test_suite::full::allocation_section(index as u32, mark);
        assert_eq!(actual, (count, 0));
        assert!(!ACTIVE.load(Ordering::Relaxed));
        total += count;
        eprintln!(
            "physical CMPI allocation section={index} count={} bad={}",
            actual.0, actual.1
        );
    }
    assert_eq!(MEASURED.load(Ordering::Relaxed) - before, total as usize);
}
