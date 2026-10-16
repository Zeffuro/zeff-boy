use super::ALLOCATIONS;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[path = "../physical_clear_not_remainder_full/mod.rs"]
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

pub(super) fn run() {
    let mut total = 0;
    for (index, expected) in full::ALLOCATION_COUNTS.into_iter().enumerate() {
        let before = MEASURED.load(Ordering::Relaxed);
        let (count, bad) = full::allocation_section(index as u32, mark);
        assert_eq!((count, bad), (expected, 0));
        assert!(!ACTIVE.load(Ordering::Relaxed));
        let intervals = MEASURED.load(Ordering::Relaxed) - before;
        assert!(intervals >= count as usize);
        total += count;
        eprintln!(
            "clear-not allocation section={index} count={count} bad={bad} intervals={intervals} delta=0"
        );
    }
    assert_eq!(total, 1421968);
    assert_eq!(MEASURED.load(Ordering::Relaxed), 2751972);
}
