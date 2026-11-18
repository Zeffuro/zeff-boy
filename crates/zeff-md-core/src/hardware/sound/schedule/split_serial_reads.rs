use super::*;

fn cartridge(profile: usize) -> Cartridge {
    let mut bytes = vec![0x69; if profile != 0 { 0x400000 } else { 0x300000 }];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x180..0x18e].copy_from_slice(
        [
            b"GM T-081276 00",
            b"GM T-081586-00",
            b"GM T-81576 -00",
            b"GM T-81476 -00",
        ][profile],
    );
    bytes[0x1b0..0x1bc].copy_from_slice(b"RA\xe8\x40\x00\x20\x00\x00\x00\x20\x00\x01");
    if profile == 2 {
        bytes[0x1b7] = 1;
    }
    Cartridge::new(bytes).unwrap()
}

fn setup(target: u32, indirect: bool) -> (Cpu, SoundMemory, SoundSchedule, Audio, u64) {
    let address = (0x8000 | (target & 0x7fff)) as u16;
    let [low, high] = address.to_le_bytes();
    let program = if indirect {
        vec![0x21, low, high, 0x7e, 0x76]
    } else {
        vec![0x3a, low, high, 0x76]
    };
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.bank = (target >> 15) as u16;
    memory.ram[..program.len()].copy_from_slice(&program);
    (
        Cpu::new(),
        memory,
        SoundSchedule::default(),
        Audio::default(),
        if indirect { 211 } else { 151 },
    )
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn split_serial_banked_reads_reject_eeprom_without_mutation_and_read_rom() {
    for profile in 0..4 {
        for target in [0x200000, 0x200001, 0x2ffffe, 0x2fffff] {
            for indirect in [false, true] {
                for eeprom in [false, true] {
                    let mut cartridge = cartridge(profile);
                    let save = cartridge.save.as_mut().unwrap();
                    assert!(save.serial_write(u16::from(!eeprom), true, true, 0, 53_693_175));
                    let before = save.clone();
                    let (mut cpu, mut memory, mut schedule, mut audio, _) = setup(target, indirect);
                    schedule.advance(
                        &mut cpu,
                        &mut memory,
                        &mut cartridge,
                        &mut [],
                        SoundInterval::new(0..2000, &mut audio),
                    );
                    if eeprom {
                        assert_eq!(
                            schedule.fault,
                            Some(SoundFault::UnsupportedAccess {
                                address: (0x8000 | (target & 0x7fff)) as u16,
                                direction: Direction::Read,
                            })
                        );
                    } else {
                        assert_eq!(schedule.fault, None);
                        assert_eq!(cpu.regs().a, 0x69);
                    }
                    assert_eq!(cartridge.save.as_ref().unwrap(), &before);
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn split_serial_pending_rom_read_rechecks_mapping_before_replay() {
    for profile in 0..4 {
        for target in [0x200000, 0x200001, 0x2ffffe, 0x2fffff] {
            for indirect in [false, true] {
                let mut cartridge = cartridge(profile);
                let (mut cpu, mut memory, mut schedule, mut audio, cut) = setup(target, indirect);
                schedule.advance(
                    &mut cpu,
                    &mut memory,
                    &mut cartridge,
                    &mut [],
                    SoundInterval::new(0..cut, &mut audio),
                );
                assert_eq!(schedule.fault, None);
                let pending = schedule.pending.as_ref().unwrap();
                assert!(pending.replay.is_some());
                let deadline = pending.deadline;
                let before_cpu = cpu.clone();
                let before_memory = memory.clone();
                let save = cartridge.save.as_mut().unwrap();
                assert!(save.serial_write(0, true, true, cut, 53_693_175));
                let before = save.clone();
                schedule.advance(
                    &mut cpu,
                    &mut memory,
                    &mut cartridge,
                    &mut [],
                    SoundInterval::new(cut..deadline - 1, &mut audio),
                );
                assert_eq!(schedule.fault, None);
                assert!(schedule.pending.is_some());
                assert_eq!(cpu, before_cpu);
                assert_eq!(memory, before_memory);
                for (begin, end) in [(deadline - 1, deadline), (deadline, deadline + 1)] {
                    schedule.advance(
                        &mut cpu,
                        &mut memory,
                        &mut cartridge,
                        &mut [],
                        SoundInterval::new(begin..end, &mut audio),
                    );
                    assert_eq!(
                        schedule.fault,
                        Some(SoundFault::UnsupportedAccess {
                            address: (0x8000 | (target & 0x7fff)) as u16,
                            direction: Direction::Read,
                        })
                    );
                    assert!(schedule.pending.is_none());
                    assert_eq!(cpu, before_cpu);
                    assert_eq!(memory, before_memory);
                    assert_eq!(cartridge.save.as_ref().unwrap(), &before);
                }
            }
        }
    }
}
