use super::*;
use crate::hardware::sound::observer::{Metric, Report, Session, Verification};

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    cpu: Cpu,
    memory: Vec<u8>,
    schedule: Vec<u8>,
    cartridge: Cartridge,
    audio: Audio,
    ram: Vec<u8>,
}

fn capture(rig: &Rig) -> Snapshot {
    let _verification = Verification::begin();
    let canonical = rig.canonical();
    let mut memory = StateWriter::new();
    canonical.memory.write_state(&mut memory).unwrap();
    let mut schedule = StateWriter::new();
    canonical
        .schedule
        .write_state(&canonical.cpu, &mut schedule)
        .unwrap();
    Snapshot {
        cpu: canonical.cpu.clone(),
        memory: memory.into_bytes(),
        schedule: schedule.into_bytes(),
        cartridge: canonical.cartridge.clone(),
        audio: canonical.audio.clone(),
        ram: canonical.ram.clone(),
    }
}

fn report(session: &Option<Session>) -> Option<Report> {
    session.as_ref().map(Session::report)
}

fn delta(before: &Option<Report>, after: &Option<Report>, metric: Metric, expected: u64) {
    if let (Some(before), Some(after)) = (before, after) {
        assert_eq!(
            after.count(metric) - before.count(metric),
            expected,
            "{metric:?}"
        );
        assert_eq!(after.flags, 0, "{after:?}");
    }
}

fn phase(epoch: &LocalEpoch, cut: u64) {
    assert_eq!(epoch.len, 8);
    assert_eq!(epoch.committed, cut as usize / 60);
    assert!(epoch.prepared && !epoch.blocked);
    for (index, record) in epoch.records.iter().enumerate() {
        let record = record.as_ref().unwrap();
        assert_eq!(record.deadline, (index as u64 + 1) * 60);
        assert_eq!(record.cpu.cycles(), (index as u64 + 1) * 4);
    }
}

fn execute(
    cut: u64,
    path: u8,
    mutate: bool,
    cached: bool,
    armed: bool,
) -> (Vec<Snapshot>, Vec<u32>, Option<Report>) {
    let session = armed.then(Session::begin);
    let mut program = vec![0; 8];
    program.extend([0x32, 0, 1, 0x76]);
    let mut rig = Rig::new(&program, cached);
    rig.memory.ram[0x100] = 0xde;
    for value in [0x84, 1, 0x90] {
        rig.audio.write_psg(0, value);
    }
    let view = if mutate {
        let view = rig.memory.ram.snapshot();
        rig.memory.ram[0x1800] = 0x5a;
        assert_eq!(view[0x1800], 0);
        Some(view)
    } else {
        None
    };
    let before_build = report(&session);
    rig.advance(0, cut, None);
    let after_build = report(&session);
    let mut retained = None;
    let deadline = (cut / 60 + 1) * 60;
    if cached {
        assert!(rig.schedule.pending.is_none());
        assert_eq!(rig.cpu.cycles(), 0);
        phase(rig.schedule.epoch.as_ref().unwrap(), cut);
        assert_eq!(before_build, after_build);
        let before_moves = report(&session);
        let cloned = rig.schedule.epoch.as_ref().unwrap().clone();
        let original = rig.schedule.epoch.take().unwrap();
        let mut slots = vec![Some(Box::new(cloned))];
        let mut slot = slots.pop().unwrap();
        let moved = *slot.take().unwrap();
        assert!(slot.is_none());
        assert_eq!(original, moved);
        phase(&moved, cut);
        rig.schedule.epoch = Some(moved);
        retained = Some(Box::new(original));
        assert_eq!(before_moves, report(&session));
        let before_materialize = report(&session);
        match path {
            0 => {
                rig.schedule.materialize_epoch(
                    &mut rig.cpu,
                    &rig.memory,
                    &mut crate::sound_trace::NoSoundCapture,
                );
                assert!(!rig.schedule.block_epoch(
                    &mut rig.cpu,
                    &rig.memory,
                    &mut crate::sound_trace::NoSoundCapture
                ));
            }
            1 => {
                let previous = rig.schedule.block_epoch(
                    &mut rig.cpu,
                    &rig.memory,
                    &mut crate::sound_trace::NoSoundCapture,
                );
                assert!(!previous);
                rig.schedule.restore_epoch(previous);
                assert!(!rig.schedule.block_epoch(
                    &mut rig.cpu,
                    &rig.memory,
                    &mut crate::sound_trace::NoSoundCapture
                ));
            }
            2 => rig.schedule.finish_epoch_field(
                &mut rig.cpu,
                &rig.memory,
                &mut crate::sound_trace::NoSoundCapture,
            ),
            _ => unreachable!(),
        }
        let after_materialize = report(&session);
        for metric in [
            Metric::OwnersCreated,
            Metric::Snapshots,
            Metric::ImagesCreated,
        ] {
            delta(&before_materialize, &after_materialize, metric, 1);
        }
        for metric in [
            Metric::Attempts,
            Metric::RamReads,
            Metric::RamWrites,
            Metric::Publications,
            Metric::DeepCopies,
            Metric::ImageClones,
        ] {
            delta(&before_materialize, &after_materialize, metric, 0);
        }
        assert_eq!(rig.cpu.cycles(), cut / 60 * 4);
        if let Some(epoch) = &rig.schedule.epoch {
            assert_eq!(
                (epoch.len, epoch.committed, epoch.prepared, epoch.blocked),
                (0, 0, false, true)
            );
        } else {
            assert_eq!(path, 2);
        }
    }
    let pending = rig.schedule.pending.as_ref().unwrap();
    assert_eq!(pending.deadline, deadline);
    assert_eq!(pending.memory.ram[0x1800], if mutate { 0x5a } else { 0 });
    let image = pending.observer_image_id();
    if armed {
        assert_ne!(image, 0);
    }
    let mut snapshots = vec![capture(&rig)];
    let mut cursor = cut;
    for end in [deadline - 1, deadline, deadline + 1, 674, 675, 676, 50_675] {
        assert!(end >= cursor);
        rig.advance(cursor, end, None);
        assert!(rig.schedule.fault.is_none());
        assert_eq!(rig.memory.ram[0x100], if end < 675 { 0xde } else { 0 });
        if end == deadline - 1 {
            assert_eq!(
                rig.schedule.pending.as_ref().unwrap().observer_image_id(),
                image
            );
        }
        if cached && end == deadline {
            let current = report(&session);
            delta(&after_build, &current, Metric::Attempts, 0);
            delta(&after_build, &current, Metric::Publications, 1);
            assert!(rig.schedule.pending.is_none());
        }
        snapshots.push(capture(&rig));
        cursor = end;
    }
    assert!(rig.schedule.pending.as_ref().unwrap().deadline > cursor);
    if let Some(epoch) = retained.as_ref() {
        phase(epoch, cut);
    }
    let before_drop = report(&session);
    drop(retained);
    assert_eq!(before_drop, report(&session));
    rig.schedule.finish_epoch_field(
        &mut rig.cpu,
        &rig.memory,
        &mut crate::sound_trace::NoSoundCapture,
    );
    rig.audio.finish_batch();
    let mut pcm = vec![];
    rig.audio.drain_into(&mut pcm);
    let pcm: Vec<_> = pcm.into_iter().map(f32::to_bits).collect();
    assert!(pcm.iter().any(|v| v & 0x7fffffff != 0));
    if let Some(view) = view.as_ref() {
        assert_eq!(view[0x1800], 0);
    }
    drop((rig, view));
    let result = session.map(Session::finish);
    if let Some(r) = &result {
        assert!(r.closed(), "{r:?}");
        assert!(!r.full_coverage);
        assert_eq!(r.scope, "DirectedOwnerThreadOnly");
        assert_eq!(r.count(Metric::DeepCopies), 0);
        assert_eq!(r.count(Metric::ImageClones), 0);
        assert_eq!(r.count(Metric::SharedDetaches), 1 + u64::from(mutate));
        assert_eq!(
            r.count(Metric::CopyBytes),
            (1 + u64::from(mutate)) * SOUND_RAM_SIZE as u64
        );
        assert_eq!(r.count(Metric::MutBorrows), 3 + u64::from(mutate));
        assert_eq!(r.count(Metric::GenerationsCreated), 2 + u64::from(mutate));
        assert_eq!(r.count(Metric::RamWrites), 1);
        assert_eq!(r.count(Metric::OneWrite), 1);
        assert_eq!(
            r.count(Metric::TwoWrites)
                + r.count(Metric::ManyWrites)
                + r.count(Metric::Failures)
                + r.count(Metric::OtherWrites)
                + r.count(Metric::OtherReads),
            0
        );
        assert_eq!(r.count(Metric::Attempts), r.count(Metric::Successes));
        assert_eq!(r.count(Metric::ZeroWrites) + 1, r.count(Metric::Successes));
        assert_eq!(
            r.count(Metric::ImagesCreated),
            r.count(Metric::Attempts) + 1
        );
        assert_eq!(
            r.count(Metric::ImagesClosed),
            r.count(Metric::ImagesCreated)
        );
        assert_eq!(
            r.count(Metric::Publications) + 1,
            r.count(Metric::ImagesCreated)
        );
        assert_eq!(
            r.count(Metric::Snapshots),
            r.count(Metric::ImagesCreated) + r.count(Metric::Publications) + u64::from(mutate)
        );
        assert_eq!(
            r.count(Metric::OwnersCreated),
            r.count(Metric::Snapshots) + 1
        );
        assert_eq!(
            r.count(Metric::OwnersClosed),
            r.count(Metric::OwnersCreated)
        );
    }
    (snapshots, pcm, result)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_epoch_retained_records_materialization_and_fallback_lifetimes() {
    let mut hash = 0xcbf29ce484222325u64;
    let mut cases = 0;
    for cut in [1, 61, 121, 479] {
        for mutate in [false, true] {
            let ordinary = execute(cut, 0, mutate, false, false);
            for path in 0..3 {
                let unarmed = execute(cut, path, mutate, true, false);
                let actual = execute(cut, path, mutate, true, true);
                assert_eq!((&actual.0, &actual.1), (&unarmed.0, &unarmed.1));
                assert_eq!((&actual.0, &actual.1), (&ordinary.0, &ordinary.1));
                for byte in format!("{cut}:{path}:{mutate}:{:?}:{:?}", actual.2, actual.1).bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                }
                cases += 1;
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("md-observer-epoch-v1 {hash:016x} cases={cases}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("md-observer-epoch-v1 {hash:016x} cases={cases}");
}
