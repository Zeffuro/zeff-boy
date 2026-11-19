use super::*;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_live_and_stale_owners_do_not_close_successor_inventory() {
    let session = Session::begin();
    let stale = SoundRam::default();
    let report = session.finish();
    assert_ne!(report.flags & INCOMPLETE, 0);
    assert_eq!(report.live_owners, 1);
    let next = Session::begin();
    let current = SoundRam::default();
    drop(stale);
    assert_eq!(next.report().live_owners, 1);
    assert_ne!(next.report().flags & INVALID_SOURCE, 0);
    drop(current);
    assert!(!next.finish().closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_missing_enrollment_and_duplicate_enrollment_are_invalid() {
    let mut ram = SoundRam::default();
    let session = Session::begin();
    ram[0] = 0x81;
    let snapshot = ram.snapshot();
    let copy = ram.clone();
    assert_ne!(session.report().flags & INVALID_SOURCE, 0);
    assert_eq!(ram[0], 0x81);
    drop((ram, snapshot, copy));
    assert!(!session.finish().closed());
    let session = Session::begin();
    let mut ram = SoundRam::default();
    ram.enroll_observer();
    drop(ram);
    assert!(!session.finish().closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_table_exhaustion_preserves_guest_allocations_and_fails_closed() {
    let session = Session::begin();
    let mut owners = Vec::new();
    for index in 0..=CAPACITY {
        let mut ram = SoundRam::default();
        ram[7] = index as u8;
        owners.push(ram);
    }
    for (index, ram) in owners.iter().enumerate() {
        assert_eq!(ram[7], index as u8);
    }
    assert_ne!(session.report().flags & EXHAUSTED, 0);
    drop(owners);
    assert!(!session.finish().closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_reentrancy_and_nesting_fail_closed_without_guest_panics() {
    let session = Session::begin();
    SLOT.with(|slot| {
        let _busy = slot.registry.borrow_mut();
        let mut ram = SoundRam::default();
        ram[0] = 0x38;
        assert_eq!(ram[0], 0x38);
        assert_ne!(session.report().flags & REENTRANT, 0);
        let nested = Session::begin();
        assert!(!nested.finish().closed());
    });
    assert_ne!(session.report().flags & REENTRANT, 0);
    assert!(!session.finish().closed());
    let session = Session::begin();
    let scopes: Vec<_> = (0..=DEPTH).map(|_| Verification::begin()).collect();
    drop(scopes);
    assert_ne!(session.report().flags & EXHAUSTED, 0);
    assert!(!session.finish().closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_checked_counters_and_attempt_order_are_not_wrapping() {
    let session = Session::begin();
    tracked(|registry| {
        registry.counters[Metric::RamReads as usize] = u64::MAX;
        registry.access(true, false);
        assert_eq!(registry.counters[Metric::RamReads as usize], u64::MAX);
        assert_ne!(registry.flags & OVERFLOW, 0);
        let first = registry.begin_attempt();
        let second = registry.begin_attempt();
        registry.end_attempt(first, true);
        assert_ne!(registry.flags & BAD_ORDER, 0);
        registry.end_attempt(second, false);
        registry.end_attempt(first, false);
    });
    assert!(!session.finish().closed());
    let allocator = std::sync::atomic::AtomicU32::new(u32::MAX - 1);
    assert_eq!(next_session(&allocator), Some(u32::MAX - 1));
    assert_eq!(next_session(&allocator), None);
    assert_eq!(next_session(&allocator), None);
    assert_eq!(
        allocator.load(std::sync::atomic::Ordering::Relaxed),
        u32::MAX
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn observer_foreign_final_drop_does_not_drain_either_session() {
    let origin = Session::begin();
    let ram = SoundRam::default();
    std::thread::spawn(move || {
        let foreign = Session::begin();
        let local = SoundRam::default();
        drop(ram);
        assert_eq!(foreign.report().live_owners, 1);
        assert_ne!(foreign.report().flags & INVALID_SOURCE, 0);
        drop(local);
        assert!(!foreign.finish().closed());
    })
    .join()
    .unwrap();
    let report = origin.finish();
    assert_eq!(report.live_owners, 1);
    assert_ne!(report.flags & INCOMPLETE, 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn observer_foreign_clone_escape_exposes_unproved_thread_coverage() {
    let session = Session::begin();
    let original = SoundRam::default();
    let (copy, snapshot) = std::thread::scope(|scope| {
        scope
            .spawn(|| (original.clone(), original.snapshot()))
            .join()
            .unwrap()
    });
    drop(original);
    let report = session.finish();
    assert!(report.closed());
    assert!(!report.full_coverage);
    assert_eq!(report.scope, "DirectedOwnerThreadOnly");
    assert_eq!(copy[0], 0);
    assert_eq!(snapshot[0], 0);
    drop((copy, snapshot));
}
