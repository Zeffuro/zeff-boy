use super::*;

pub(super) fn contract() {
    let mut bounded = cartridge(8192);
    bounded.save = Some(crate::cartridge::SaveMemory::bounded_odd_for_test());
    for target in [
        0x200000, 0x200001, 0x2012bf, 0x2012c1, 0x3fffff, 0xa13001, 0xa130f1,
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
    for address in [0x200001, 0x2012c1, 0x3fffff, 0xa13001, 0xa130f1] {
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
                        pending.replay.as_mut().unwrap().bank_identity = Some((address, direction))
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
