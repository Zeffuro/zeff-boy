use super::*;
use crate::constants::SOUND_RAM_SIZE;
use crate::hardware::sound::SoundMemory;

fn checkpoint(cpu: &Cpu, memory: &SoundMemory, images: &[&PendingSound], audio: &Audio) -> String {
    format!("{cpu:?}|{memory:?}|{images:?}|{audio:?}")
}

fn execute(outcome: u8, order: usize, armed: bool) -> (Vec<String>, Vec<u32>, Option<Report>) {
    let session = armed.then(Session::begin);
    let mut cartridge = cartridge();
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.ram[..4].copy_from_slice(&[0x3a, 0, 0x40, 0x76]);
    memory.bank = 0x140;
    let mut cpu = Cpu::new();
    let mut audio = Audio::default();
    for value in [0x84, 1, 0x90] {
        audio.write_psg(0, value);
    }
    audio.write_opn2(0, 0, 0x2a);
    audio.write_opn2(151, 1, 0x90);
    let original = PendingSound::prepare(&cpu, &memory, &cartridge, 0).unwrap();
    assert_eq!(original.deadline, 195);
    let retained = original.clone();
    let mut candidate = retained.clone();
    let ids = [
        original.observer_image_id(),
        retained.observer_image_id(),
        candidate.observer_image_id(),
    ];
    if armed {
        assert!(ids.iter().all(|id| *id != 0));
        assert!(ids[0] != ids[1] && ids[0] != ids[2] && ids[1] != ids[2]);
    }
    memory.write(0x777, 0xa6);
    memory.bank = 0x1ff;
    memory.irq_asserted = true;
    match outcome {
        0 => {}
        1 => memory.ram[0] = 0,
        2 => memory.ram[..3].copy_from_slice(&[0x3a, 0, 0x7f]),
        3 => candidate.deadline += 1,
        _ => unreachable!(),
    }
    let before = {
        let _verification = Verification::begin();
        (candidate.clone(), memory.clone(), audio.clone())
    };
    let mut snapshots = vec![checkpoint(
        &cpu,
        &memory,
        &[&original, &retained, &candidate],
        &audio,
    )];
    let result = candidate.refresh_reads(&cpu, &memory, &cartridge, &audio);
    assert_eq!(memory, before.1);
    assert_eq!(audio, before.2);
    assert_eq!(cpu, Cpu::new());
    match outcome {
        0 => {
            assert_eq!(result, Ok(()));
            assert_eq!(candidate.cpu.regs().a, 0x80);
            assert_eq!(candidate.memory.ram[0x777], 0xa6);
            assert_eq!(candidate.memory.bank, 0x140);
            assert!(!candidate.memory.irq_asserted);
            if armed {
                assert_ne!(candidate.observer_image_id(), ids[2]);
            }
        }
        _ => {
            assert_eq!(
                result,
                Err(match outcome {
                    1 => crate::SoundFault::UnstableOpn2Read { pc: 0 },
                    2 => crate::SoundFault::UnsupportedAccess {
                        address: 0x7f00,
                        direction: zeff_m68k::bus::Direction::Read,
                    },
                    3 => crate::SoundFault::ClockOverflow,
                    _ => unreachable!(),
                })
            );
            assert_eq!(candidate, before.0);
            assert_eq!(candidate.observer_image_id(), ids[2]);
        }
    }
    assert_eq!(original, retained);
    assert_eq!(original.memory.ram[0x777], 0);
    assert_eq!(original.cpu.regs().a, 0);
    assert_eq!(original.observer_image_id(), ids[0]);
    assert_eq!(retained.observer_image_id(), ids[1]);
    snapshots.push(checkpoint(
        &cpu,
        &memory,
        &[&original, &retained, &candidate],
        &audio,
    ));
    if outcome == 0 {
        for _ in 0..2 {
            candidate.publish(&mut cpu, &mut memory, &mut [], &mut audio, &mut cartridge);
            assert_eq!(cpu.regs().a, 0x80);
            assert_eq!(memory.ram[0x777], 0xa6);
            assert_eq!(memory.bank, 0x140);
            assert!(memory.irq_asserted);
            snapshots.push(checkpoint(&cpu, &memory, &[&candidate], &audio));
        }
    }
    audio.advance_to(50_000);
    let mut pcm = vec![];
    audio.drain_into(&mut pcm);
    let pcm: Vec<_> = pcm.into_iter().map(f32::to_bits).collect();
    assert!(pcm.iter().any(|value| value & 0x7fffffff != 0));
    {
        let _verification = Verification::begin();
        drop(before);
    }
    let mut live = Some(memory);
    let mut images = [Some(original), Some(retained), Some(candidate)];
    if order == 0 {
        drop(live.take());
    }
    for index in match order {
        0 | 1 => [0, 1, 2],
        2 => [2, 1, 0],
        _ => unreachable!(),
    } {
        let before_drop = session.as_ref().map(Session::report);
        drop(images[index].take());
        if let Some(before_drop) = before_drop {
            let after = session.as_ref().unwrap().report();
            assert_eq!(after.flags, 0);
            assert_eq!(
                after.count(Metric::ImagesClosed),
                before_drop.count(Metric::ImagesClosed) + 1
            );
            assert_eq!(
                after.count(Metric::OwnersClosed),
                before_drop.count(Metric::OwnersClosed) + 1
            );
            assert_eq!(
                after.count(Metric::Publications),
                before_drop.count(Metric::Publications)
            );
        }
    }
    drop((live, images, audio, cpu, cartridge));
    let report = session.map(Session::finish);
    if let Some(report) = &report {
        let published = u64::from(outcome == 0) * 2;
        let unsupported = u64::from(outcome == 2);
        assert!(report.closed(), "{report:?}");
        assert!(!report.full_coverage);
        assert_eq!(report.scope, "DirectedOwnerThreadOnly");
        for (metric, expected) in [
            (Metric::OwnersCreated, 6 + published),
            (Metric::OwnersClosed, 6 + published),
            (Metric::GenerationsCreated, 4),
            (Metric::Snapshots, 3 + published),
            (Metric::DeepCopies, 2),
            (Metric::MutBorrows, 2 + u64::from(matches!(outcome, 1 | 2))),
            (Metric::SharedDetaches, 1),
            (Metric::WeakDissociations, 0),
            (Metric::CopyBytes, 3 * SOUND_RAM_SIZE as u64),
            (Metric::ImagesCreated, 4),
            (Metric::ImagesClosed, 4),
            (Metric::ImageClones, 2),
            (Metric::Attempts, 2),
            (Metric::Successes, 2 - unsupported),
            (Metric::Failures, unsupported),
            (Metric::Publications, published),
            (Metric::RamReads, if outcome == 1 { 4 } else { 6 }),
            (Metric::RamWrites, 0),
            (Metric::OtherReads, if outcome == 1 { 1 } else { 2 }),
            (Metric::OtherWrites, 0),
            (Metric::ZeroWrites, 2 - unsupported),
            (Metric::OneWrite, 0),
            (Metric::TwoWrites, 0),
            (Metric::ManyWrites, 0),
        ] {
            assert_eq!(report.count(metric), expected, "{metric:?}: {report:?}");
        }
    }
    (snapshots, pcm, report)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_retained_images_refresh_outcomes_and_cleanup_orders() {
    let mut hash = 0xcbf29ce484222325_u64;
    let mut cases = 0;
    for outcome in 0..4 {
        for order in 0..3 {
            let expected = execute(outcome, order, false);
            let actual = execute(outcome, order, true);
            assert_eq!((&actual.0, &actual.1), (&expected.0, &expected.1));
            for byte in format!("{outcome}:{order}:{:?}:{:?}", actual.2, actual.1).bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
            cases += 1;
        }
    }
    super::super::super::observer_contract_tests::emit(format!(
        "md-observer-retained-v1 {hash:016x} cases={cases}"
    ));
}
