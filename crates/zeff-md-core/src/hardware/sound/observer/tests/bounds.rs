use super::*;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_bounds_owner_table_exhausts_with_one_generation() {
    let session = Session::begin();
    let ram = SoundRam::default();
    let mut snapshots = Vec::new();
    for _ in 1..CAPACITY {
        snapshots.push(ram.snapshot());
    }
    let before = session.report();
    assert_eq!(before.flags, 0);
    assert_eq!(before.live_owners, CAPACITY);
    assert_eq!(before.count(Metric::GenerationsCreated), 1);
    let mut overflow = ram.snapshot();
    assert_ne!(session.report().flags & EXHAUSTED, 0);
    overflow[5] = 0x93;
    assert_eq!(overflow[5], 0x93);
    assert_eq!(ram[5], 0);
    drop((overflow, snapshots, ram));
    let report = session.finish();
    assert_eq!(report.live_owners, 0);
    assert_eq!(report.count(Metric::OwnersClosed), CAPACITY as u64);
    assert!(!report.closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_bounds_generation_records_reuse_after_weak_dissociation() {
    let session = Session::begin();
    let mut ram = SoundRam::default();
    for index in 1..3 * CAPACITY {
        let weak = ram.observer_weak();
        ram[5] = index as u8;
        assert!(weak.upgrade().is_none());
    }
    let report = session.report();
    assert_eq!(report.flags, 0);
    assert_eq!(report.live_owners, 1);
    assert_eq!(report.count(Metric::OwnersCreated), 1);
    assert_eq!(
        report.count(Metric::GenerationsCreated),
        (3 * CAPACITY) as u64
    );
    assert_eq!(report.count(Metric::CopyBytes), 0);
    drop(ram);
    assert!(session.finish().closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_bounds_image_records_reuse_then_reject_live_capacity() {
    let session = Session::begin();
    let ram = SoundRam::default();
    for _ in 0..3 * CAPACITY {
        let image = image(&ram.1, None);
        publication(&image);
        drop(image);
    }
    assert_eq!(session.report().flags, 0);
    let images: Vec<_> = (0..CAPACITY).map(|_| image(&ram.1, None)).collect();
    assert_eq!(session.report().live_images, CAPACITY);
    let overflow = image(&ram.1, None);
    assert_eq!(overflow.id, 0);
    assert_ne!(session.report().flags & EXHAUSTED, 0);
    drop((overflow, images, ram));
    let report = session.finish();
    assert_eq!(report.count(Metric::ImagesCreated), (4 * CAPACITY) as u64);
    assert_eq!(report.count(Metric::ImagesClosed), (4 * CAPACITY) as u64);
    assert_eq!((report.live_owners, report.live_images), (0, 0));
    assert!(!report.closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_bounds_preparation_records_reuse_after_each_completed_attempt() {
    let session = Session::begin();
    for _ in 0..3 * CAPACITY {
        let mut attempt = Attempt::begin();
        attempt.succeed();
    }
    let report = session.finish();
    assert!(report.closed());
    assert_eq!(report.count(Metric::Attempts), (3 * CAPACITY) as u64);
    assert_eq!(report.count(Metric::Successes), (3 * CAPACITY) as u64);
    assert_eq!(report.count(Metric::ZeroWrites), (3 * CAPACITY) as u64);
    assert_eq!(report.count(Metric::Failures), 0);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_bounds_attempt_depth_unwinds_all_accepted_attempts() {
    let session = Session::begin();
    let mut attempts: Vec<_> = (0..DEPTH).map(|_| Attempt::begin()).collect();
    assert_eq!(session.report().flags, 0);
    let overflow = Attempt::begin();
    assert_ne!(session.report().flags & EXHAUSTED, 0);
    drop(overflow);
    while let Some(mut attempt) = attempts.pop() {
        attempt.succeed();
    }
    tracked(|registry| assert_eq!(registry.attempt_depth, 0));
    let report = session.finish();
    assert_eq!(report.count(Metric::Attempts), DEPTH as u64);
    assert_eq!(report.count(Metric::Successes), DEPTH as u64);
    assert!(!report.closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_bounds_clone_depth_preserves_copy_and_unwinds_accepted_scopes() {
    let session = Session::begin();
    let ram = SoundRam::from([0x93; crate::constants::SOUND_RAM_SIZE]);
    let mut scopes: Vec<_> = (0..DEPTH).map(|_| deep_clone(&ram.1)).collect();
    assert_eq!(session.report().flags, 0);
    let mut copy = ram.clone();
    assert_ne!(session.report().flags & EXHAUSTED, 0);
    copy[5] = 0x38;
    assert_eq!(copy[5], 0x38);
    assert_eq!(ram[5], 0x93);
    while let Some(scope) = scopes.pop() {
        drop(scope);
    }
    tracked(|registry| assert_eq!(registry.parent_depth, 0));
    drop((copy, ram));
    let report = session.finish();
    assert_eq!(report.live_owners, 0);
    assert_eq!(report.count(Metric::DeepCopies), 1);
    assert_eq!(
        report.count(Metric::CopyBytes),
        crate::constants::SOUND_RAM_SIZE as u64
    );
    assert!(!report.closed());
}
