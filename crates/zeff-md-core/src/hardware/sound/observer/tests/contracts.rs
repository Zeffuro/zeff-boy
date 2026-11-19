use super::super::token::Kind;
use super::*;
use crate::constants::SOUND_RAM_SIZE;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_ram_owners_generations_clone_and_single_cow() {
    let session = Session::begin();
    {
        let mut ram = SoundRam::from([0x27; SOUND_RAM_SIZE]);
        let snapshot = ram.snapshot();
        let copy = ram.clone();
        assert_ne!(ram.1.id, snapshot.1.id);
        assert_ne!(ram.1.id, copy.1.id);
        tracked(|registry| {
            assert_eq!(
                registry.owners.get(ram.1.id).unwrap().generation,
                registry.owners.get(snapshot.1.id).unwrap().generation
            );
            assert_ne!(
                registry.owners.get(ram.1.id).unwrap().generation,
                registry.owners.get(copy.1.id).unwrap().generation
            );
        });
        assert_eq!(ram, snapshot);
        assert_eq!(ram, copy);
        assert_eq!(format!("{ram:?}"), format!("SoundRam({:?})", &*ram));
        ram[7] = 0x51;
        assert_eq!(snapshot[7], 0x27);
        assert_eq!(copy[7], 0x27);
        let report = session.report();
        assert_eq!(report.count(Metric::OwnersCreated), 3);
        assert_eq!(report.count(Metric::Snapshots), 1);
        assert_eq!(report.count(Metric::DeepCopies), 1);
        assert_eq!(report.count(Metric::SharedDetaches), 1);
        assert_eq!(report.count(Metric::GenerationsCreated), 3);
        assert_eq!(report.count(Metric::CopyBytes), 2 * SOUND_RAM_SIZE as u64);
    }
    assert!(session.finish().closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_weak_dissociation_is_not_a_shared_copy() {
    let session = Session::begin();
    {
        let mut ram = SoundRam::default();
        let weak = ram.observer_weak();
        ram[3] = 0x81;
        assert!(weak.upgrade().is_none());
        assert_eq!(ram[3], 0x81);
        let report = session.report();
        assert_eq!(report.count(Metric::WeakDissociations), 1);
        assert_eq!(report.count(Metric::SharedDetaches), 0);
        assert_eq!(report.count(Metric::CopyBytes), 0);
    }
    assert!(session.finish().closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_pending_clone_box_moves_and_borrowed_repeat_publication() {
    let session = Session::begin();
    {
        let mut cartridge = cartridge();
        let mut memory = memory();
        let mut cpu = Cpu::new();
        let mut audio = Audio::default();
        let pending = PendingSound::prepare(&cpu, &memory, &cartridge, 0).unwrap();
        let copy = pending.clone();
        assert_ne!(pending.observer_image_id(), copy.observer_image_id());
        assert_eq!(pending, copy);
        assert_eq!(format!("{pending:?}"), format!("{copy:?}"));
        let id = copy.observer_image_id();
        let mut boxed = Some(Box::new(copy));
        let moved = *boxed.take().unwrap();
        assert_eq!(moved.observer_image_id(), id);
        for _ in 0..2 {
            moved.publish(&mut cpu, &mut memory, &mut [], &mut audio, &mut cartridge);
        }
        let report = session.report();
        assert_eq!(report.count(Metric::Attempts), 1);
        assert_eq!(report.count(Metric::ImageClones), 1);
        assert_eq!(report.count(Metric::DeepCopies), 1);
        assert_eq!(report.count(Metric::Publications), 2);
        assert_eq!(report.count(Metric::ImagesClosed), 0);
    }
    let report = session.finish();
    assert!(report.closed(), "{report:?}");
    assert_eq!(report.count(Metric::ImagesClosed), 2);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_failed_cpu_preparation_keeps_failure_and_natural_disposal() {
    let session = Session::begin();
    {
        let mut memory = memory();
        memory.ram[..3].copy_from_slice(&[0x3a, 0, 0x7f]);
        assert!(matches!(
            PendingSound::prepare(&Cpu::new(), &memory, &cartridge(), 0),
            Err(crate::SoundFault::UnsupportedAccess {
                address: 0x7f00,
                direction: zeff_m68k::bus::Direction::Read
            })
        ));
        let report = session.report();
        assert_eq!(report.count(Metric::Attempts), 1);
        assert_eq!(report.count(Metric::Failures), 1);
        assert_eq!(report.count(Metric::Successes), 0);
        assert_eq!(report.count(Metric::ImagesCreated), 1);
        assert_eq!(report.count(Metric::ImagesClosed), 1);
        assert_eq!(report.count(Metric::Publications), 0);
    }
    assert!(session.finish().closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_active_whole_raw20_raw21_frontend1_remain_neutral() {
    let expected = super::super::super::observer_contract_tests::whole_versions();
    let session = Session::begin();
    assert_eq!(
        super::super::super::observer_contract_tests::whole_versions(),
        expected
    );
    let report = session.finish();
    assert!(report.closed(), "{report:?}");
    assert!(report.count(Metric::DeepCopies) > 0);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_reuse_retains_pending_checkpoint_across_inventory_churn() {
    fn execute() -> (String, Vec<u32>) {
        let mut cartridge = cartridge();
        let mut memory = memory();
        let mut cpu = Cpu::new();
        let mut audio = Audio::default();
        let pending = PendingSound::prepare(&cpu, &memory, &cartridge, 0).unwrap();
        let checkpoint = pending.clone();
        let retained = (pending.observer_image_id(), checkpoint.observer_image_id());
        for index in 0..3 * CAPACITY {
            let fresh = PendingSound::prepare(&cpu, &memory, &cartridge, 0).unwrap();
            let mut snapshot = memory.ram.snapshot();
            snapshot[7] = index as u8;
            drop((fresh, snapshot));
        }
        assert_eq!(
            retained,
            (pending.observer_image_id(), checkpoint.observer_image_id())
        );
        let copy = checkpoint.clone();
        for image in [&pending, &checkpoint, &copy, &checkpoint] {
            image.publish(&mut cpu, &mut memory, &mut [], &mut audio, &mut cartridge);
        }
        let _verification = Verification::begin();
        let signature = format!("{cpu:?}|{memory:?}|{pending:?}|{checkpoint:?}|{copy:?}");
        let mut pcm = vec![];
        audio.drain_into(&mut pcm);
        (signature, pcm.into_iter().map(f32::to_bits).collect())
    }
    let expected = execute();
    let session = Session::begin();
    assert_eq!(execute(), expected);
    let report = session.finish();
    assert!(report.closed(), "{report:?}");
    assert!(report.count(Metric::OwnersCreated) > (3 * CAPACITY) as u64);
    assert!(report.count(Metric::GenerationsCreated) > CAPACITY as u64);
    assert!(report.count(Metric::ImagesCreated) > CAPACITY as u64);
    assert!(report.count(Metric::Attempts) > CAPACITY as u64);
    assert_eq!(report.count(Metric::Publications), 4);
    assert_eq!(report.count(Metric::ImageClones), 2);
    assert!(!report.full_coverage);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_reuse_stale_owner_image_and_attempt_cannot_touch_replacements() {
    let session = Session::begin();
    tracked(|registry| {
        let old = registry.allocation(1);
        let stale = Token::new(old.session, old.id, Kind::Owner);
        registry.close(&old);
        let current = registry.allocation(2);
        assert_ne!(old.id, current.id);
        assert_eq!(registry.source(&stale), None);
        registry.close(&stale);
        assert!(registry.source(&current).is_some());
        let image = registry.image(&current, None);
        registry.close(&image);
        let replacement = registry.image(&current, None);
        assert_ne!(image.id, replacement.id);
        registry.publication(&image);
        registry.close(&image);
        assert_eq!(registry.report().live_images, 1);
        registry.publication(&replacement);
        let ended = registry.begin_attempt();
        registry.end_attempt(ended, true);
        let active = registry.begin_attempt();
        assert_ne!(ended, active);
        registry.end_attempt(ended, true);
        assert_eq!(registry.attempt_depth, 1);
        registry.end_attempt(active, true);
        registry.close(&current);
        registry.close(&replacement);
        assert_eq!(
            (registry.report().live_owners, registry.report().live_images),
            (0, 0)
        );
        assert_eq!(registry.counters[Metric::OwnersClosed as usize], 2);
        assert_eq!(registry.counters[Metric::ImagesClosed as usize], 2);
        assert_ne!(registry.flags & INVALID_SOURCE, 0);
        assert_ne!(registry.flags & BAD_ORDER, 0);
        std::mem::forget((old, stale, current, image, replacement));
    });
    assert!(!session.finish().closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_reuse_admission_reclaims_generation_after_owner_capacity_rejection() {
    let session = Session::begin();
    let ram = SoundRam::default();
    let mut snapshots: Vec<_> = (1..CAPACITY).map(|_| ram.snapshot()).collect();
    assert_eq!(session.report().live_owners, CAPACITY);
    for index in 0..3 * CAPACITY {
        let mut extra = SoundRam::default();
        assert_eq!(extra.1.id, 0);
        extra[7] = index as u8;
        assert_eq!(extra[7], index as u8);
        assert_eq!(ram[7], 0);
    }
    assert_ne!(session.report().flags & EXHAUSTED, 0);
    drop(snapshots.pop().unwrap());
    let recovered = SoundRam::default();
    assert_ne!(recovered.1.id, 0);
    assert_eq!(session.report().live_owners, CAPACITY);
    drop((recovered, snapshots, ram));
    let report = session.finish();
    assert_eq!(report.live_owners, 0);
    assert_eq!(report.count(Metric::OwnersCreated), (CAPACITY + 1) as u64);
    assert_eq!(
        report.count(Metric::OwnersCreated),
        report.count(Metric::OwnersClosed)
    );
    assert!(!report.closed());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_reuse_admission_nested_attempts_keep_separate_write_categories() {
    let session = Session::begin();
    for _ in 0..3 * CAPACITY {
        let mut outer = Attempt::begin();
        access(0, zeff_m68k::bus::Direction::Write);
        {
            let mut inner = Attempt::begin();
            access(0, zeff_m68k::bus::Direction::Write);
            access(0, zeff_m68k::bus::Direction::Write);
            inner.succeed();
        }
        access(0, zeff_m68k::bus::Direction::Write);
        outer.succeed();
    }
    let report = session.finish();
    assert!(report.closed());
    assert_eq!(report.count(Metric::Attempts), (6 * CAPACITY) as u64);
    assert_eq!(report.count(Metric::Successes), (6 * CAPACITY) as u64);
    assert_eq!(report.count(Metric::TwoWrites), (6 * CAPACITY) as u64);
    assert_eq!(report.count(Metric::RamWrites), (12 * CAPACITY) as u64);
    assert_eq!(report.count(Metric::Failures), 0);
}
