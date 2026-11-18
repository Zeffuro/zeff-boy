use super::*;

pub(super) fn contract() {
    for (profile, mode) in [(0, 0), (0, 1), (0, 3), (1, 0), (2, 0), (2, 1), (2, 3)] {
        let mut bounded = cartridge(8192);
        let mut save = if profile == 2 {
            crate::cartridge::SaveMemory::bounded_bytes_for_test()
        } else if profile == 0 {
            crate::cartridge::SaveMemory::bounded_switched_for_test()
        } else {
            crate::cartridge::SaveMemory::reserved_switched_for_test()
        };
        if profile != 1 {
            assert!(save.control(mode, false));
        }
        bounded.save = Some(save);
        for target in [
            0x200000, 0x200005, 0x2007ff, 0x200800, 0x203fff, 0x204001, 0x3fffff, 0xa13001,
            0xa130f1,
        ] {
            for write in [false, true] {
                let (cpu, mut memory, _, _, _) = setup(target, false);
                if write {
                    memory.ram[0] = 0x32;
                }
                let before = bounded.clone();
                assert!(matches!(
                    PendingSound::prepare(&cpu, &memory, &bounded, 0),
                    Err(SoundFault::UnsupportedAccess { .. })
                ));
                assert_eq!(bounded, before);
            }
        }
        let (mut cpu, mut memory, mut schedule, mut audio, cut) = setup(0x200001, false);
        let mut ordinary = cartridge(8192);
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut ordinary,
            &mut [],
            SoundInterval::new(0..cut, &mut audio),
        );
        let original = schedule.pending.take().unwrap();
        for address in [
            0x200000, 0x200001, 0x2007ff, 0x200800, 0x203fff, 0x204001, 0x3fffff, 0xa13001,
            0xa130f1,
        ] {
            for direction in [Direction::Read, Direction::Write] {
                for slot in 0..3 {
                    let mut pending = original.clone();
                    pending.bank_access = None;
                    pending.observed_bank_access = None;
                    pending.replay.as_mut().unwrap().bank_identity = None;
                    let access = BankAccess {
                        address,
                        direction,
                        value: 0xa5,
                    };
                    match slot {
                        0 => pending.bank_access = Some(access),
                        1 => pending.observed_bank_access = Some(access),
                        _ => {
                            pending.replay.as_mut().unwrap().bank_identity =
                                Some((address, direction))
                        }
                    }
                    schedule.pending = Some(pending);
                    let before = schedule.clone();
                    assert!(schedule.validate_cartridge(&bounded).is_err());
                    assert_eq!(schedule, before);
                }
            }
        }
    }
}
