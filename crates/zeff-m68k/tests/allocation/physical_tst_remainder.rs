use super::ALLOCATIONS;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[path = "../physical_tst_remainder_full/mod.rs"]
mod fixture;

const COUNTS: [u32; 8] = [162_600, 22_098, 10_160, 23_760, 528, 20_262, 5_484, 36_576];
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

pub(super) fn tst_remainder_steps() {
    let before = MEASURED.load(Ordering::Relaxed);
    let mut total = 0;
    for (index, expected) in COUNTS.into_iter().enumerate() {
        let (count, bad) = fixture::allocation_section(index as u32, mark);
        assert_eq!((count, bad), (expected, 0));
        assert!(!ACTIVE.load(Ordering::Relaxed));
        total += count;
        eprintln!("physical TST allocation section={index} count={count} bad={bad}");
    }
    assert_eq!(total, 281_468);
    assert_eq!(MEASURED.load(Ordering::Relaxed) - before, total as usize);
}
