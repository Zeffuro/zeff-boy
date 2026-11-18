use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn cartridge(size: usize) -> Cartridge {
    let mut bytes = vec![0x69; size];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    Cartridge::new(bytes).unwrap()
}

fn save_cartridge(profile: u8) -> Cartridge {
    let size = if profile == 1 { 0x300000 } else { 0x100000 };
    let mut bytes = cartridge(size).bytes.to_vec();
    if profile == 2 {
        bytes[0x180..0x18e].copy_from_slice(b"MK 00001211-00");
        bytes[0x1b0..0x1bc].copy_from_slice(b"RA\xe8\x40\x00\x03\x0d\x41\x00\x03\x0d\x41");
    } else {
        if profile == 1 {
            bytes[0x180..0x18e].copy_from_slice(b"GM G-5524  -00");
        }
        bytes[0x1b0..0x1bc]
            .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 0x3f, 0xff]);
    }
    Cartridge::new(bytes).unwrap()
}

#[test]
fn linear_rom_writes_capture_every_bank_without_changing_memory_or_journals() {
    let cartridge = cartridge(0x100000);
    let before = cartridge.clone();
    for bank in 0..=0x7f {
        for address in [0x8000, 0x8001, 0x806a, 0xfffe, 0xffff] {
            for value in [0, 0x42, 0xff] {
                let mut memory = SoundMemory {
                    bank,
                    ..SoundMemory::default()
                };
                let saved_memory = memory.clone();
                let target = (u32::from(bank) << 15) | u32::from(address & 0x7fff);
                let mut candidate = CandidateBus::new(&mut memory, &cartridge);
                candidate.cpu_write(address, value);
                assert_eq!(candidate.fault.get(), None);
                assert_eq!(
                    candidate.bank_access.get(),
                    Some(BankAccess {
                        address: target,
                        direction: Direction::Write,
                        value,
                    })
                );
                assert_eq!(candidate.accesses.get(), 1);
                assert!(!candidate.timed_read.get());
                assert_eq!(candidate.cartridge_read.get(), None);
                assert_eq!(candidate.writes, WorkWrites::default());
                assert_eq!(candidate.psg_writes, PsgWrites::default());
                assert_eq!(candidate.opn2_writes, Opn2Writes::default());
                assert_eq!(memory, saved_memory);
                let reader = CandidateBus::new(&mut memory, &cartridge);
                let expected = cartridge.read(target).unwrap();
                assert_eq!(reader.cpu_read(address), expected);
                assert_eq!(reader.fault.get(), None);
                assert_eq!(
                    reader.bank_access.get(),
                    Some(BankAccess {
                        address: target,
                        direction: Direction::Read,
                        value: expected,
                    })
                );
            }
        }
    }
    assert_eq!(cartridge, before);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn rom_write_admission_retains_all_save_and_neighbor_rejections() {
    for profile in 0..=2 {
        let mut cartridge = save_cartridge(profile);
        for enabled in [false, true] {
            if profile != 1 && !enabled {
                continue;
            }
            cartridge.save.as_mut().unwrap().enabled = enabled;
            for target in [0x200000, 0x200001, 0x203fff] {
                assert!(cartridge.save.as_ref().unwrap().contains(target));
                let before = cartridge.clone();
                let mut memory = SoundMemory {
                    bank: (target >> 15) as u16,
                    ..SoundMemory::default()
                };
                let saved_memory = memory.clone();
                let address = (0x8000 | (target & 0x7fff)) as u16;
                let mut candidate = CandidateBus::new(&mut memory, &cartridge);
                candidate.cpu_write(address, 0xff);
                assert_eq!(
                    candidate.fault.get(),
                    Some(SoundFault::UnsupportedAccess {
                        address,
                        direction: Direction::Write,
                    })
                );
                assert_eq!(candidate.bank_access.get(), None);
                assert_eq!(candidate.writes, WorkWrites::default());
                assert_eq!(candidate.psg_writes, PsgWrites::default());
                assert_eq!(candidate.opn2_writes, Opn2Writes::default());
                assert_eq!(memory, saved_memory);
                assert_eq!(cartridge, before);
            }
            let mut memory = SoundMemory::default();
            let mut candidate = CandidateBus::new(&mut memory, &cartridge);
            candidate.cpu_write(0x8000, 0xff);
            assert_eq!(candidate.fault.get(), None);
        }
    }
    let cartridge = cartridge(0x100000);
    for target in [
        0x400000, 0x407fff, 0x7fffff, 0x800000, 0x9fffff, 0xa00000, 0xa04004, 0xa05fff, 0xa06000,
        0xa10000, 0xa17fff, 0xbfffff, 0xc00018, 0xc80019, 0xdfffff,
    ] {
        let address = (0x8000 | (target & 0x7fff)) as u16;
        let mut memory = SoundMemory {
            bank: (target >> 15) as u16,
            ..SoundMemory::default()
        };
        let saved_memory = memory.clone();
        let mut candidate = CandidateBus::new(&mut memory, &cartridge);
        candidate.cpu_write(address, 0x42);
        assert_eq!(
            candidate.fault.get(),
            Some(SoundFault::UnsupportedAccess {
                address,
                direction: Direction::Write,
            })
        );
        assert_eq!(candidate.bank_access.get(), None);
        assert_eq!(memory, saved_memory);
    }
}

#[test]
fn ignored_rom_write_waits_publish_cpu_only_at_deadline_and_restore_exactly() {
    for target in [0, 0x7fff, 0x0f006a, 0x100000, 0x3fffff] {
        for requester in [Requester::MAIN, Requester::VDP] {
            let mut cartridge = cartridge(0x100000);
            let before = cartridge.clone();
            let address = (0x8000 | (target & 0x7fff)) as u16;
            let [low, high] = address.to_le_bytes();
            let mut memory = SoundMemory::default();
            memory.set_reset(false);
            memory.bank = (target >> 15) as u16;
            memory.ram[..4].copy_from_slice(&[0x32, low, high, 0x76]);
            let mut cpu = Cpu::new();
            let mut schedule = SoundSchedule::default();
            let mut audio = Audio::default();
            let mut ram = vec![0xde; WORK_RAM_MASK + 1];
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cartridge,
                &mut ram,
                SoundInterval::new(0..151, &mut audio),
            );
            assert_eq!(schedule.fault, None);
            let pending = schedule.pending.as_ref().unwrap();
            assert_eq!((pending.start, pending.deadline), (150, 195));
            assert_eq!(
                pending.bank_access,
                Some(BankAccess {
                    address: target,
                    direction: Direction::Write,
                    value: 0,
                })
            );
            assert!(pending.any_bank_access);
            assert!(pending.replay.is_none());
            assert_eq!(pending.reservation().unwrap().ticks, 150..195);
            assert!(!schedule.local_pending_after(151));
            assert_eq!(cpu.cycles(), 10);
            let mut writer = StateWriter::new();
            schedule.write_state(&cpu, &mut writer).unwrap();
            let saved = writer.into_bytes();
            let mut restored = SoundSchedule::default();
            let mut restored_cpu = Cpu::new();
            let mut reader = StateReader::new(&saved);
            restored.read_state(&mut restored_cpu, &mut reader).unwrap();
            assert!(reader.is_exhausted());
            assert_eq!(restored, schedule);
            assert_eq!(restored_cpu, cpu);
            memory.bank = 0x100;
            let mut restored_memory = memory.clone();
            let mut restored_audio = audio.clone();
            let mut restored_ram = ram.clone();
            let occupied = [Reservation {
                requester,
                ticks: 150..1500,
            }];
            for (cpu, memory, schedule, audio, ram) in [
                (&mut cpu, &mut memory, &mut schedule, &mut audio, &mut ram),
                (
                    &mut restored_cpu,
                    &mut restored_memory,
                    &mut restored,
                    &mut restored_audio,
                    &mut restored_ram,
                ),
            ] {
                schedule.advance(
                    cpu,
                    memory,
                    &mut cartridge,
                    ram,
                    SoundInterval {
                        ticks: 151..1544,
                        occupied: &occupied,
                        sound_irq: None,
                        audio,
                    },
                );
                assert_eq!(schedule.fault, None);
                assert_eq!(cpu.cycles(), 10);
                let pending = schedule.pending.as_ref().unwrap();
                assert_eq!((pending.start, pending.deadline), (1500, 1545));
                assert_eq!(pending.reservation().unwrap().ticks, 1500..1545);
                schedule.advance(
                    cpu,
                    memory,
                    &mut cartridge,
                    ram,
                    SoundInterval::new(1544..1545, audio),
                );
                assert_eq!(schedule.fault, None);
                assert_eq!(cpu.cycles(), 103);
                assert_eq!(memory.bank, (target >> 15) as u16);
                assert!(schedule.pending.is_none());
                assert!(ram.iter().all(|value| *value == 0xde));
            }
            assert_eq!(cpu, restored_cpu);
            assert_eq!(memory, restored_memory);
            assert_eq!(schedule, restored);
            assert_eq!(audio, restored_audio);
            assert_eq!(ram, restored_ram);
            assert_eq!(cartridge, before);
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn indexed_rom_write_and_restart_stack_writes_keep_effective_bank_identity() {
    for (program, bank, cut, target, value) in [
        (
            vec![0xdd, 0x21, 0x30, 0x80, 0xdd, 0x36, 0x3a, 0xff, 0x76],
            0x1e,
            451,
            0x0f006a,
            0xff,
        ),
        (vec![0x31, 0, 0, 0xff, 0x76], 0, 226, 0x7fff, 0),
    ] {
        let mut cartridge = cartridge(0x100000);
        let before = cartridge.clone();
        let mut memory = SoundMemory::default();
        memory.set_reset(false);
        memory.bank = bank;
        memory.ram[..program.len()].copy_from_slice(&program);
        let mut cpu = Cpu::new();
        let mut audio = Audio::default();
        let mut schedule = SoundSchedule::default();
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(0..cut, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        let pending = schedule.pending.as_ref().unwrap();
        assert!(pending.any_bank_access);
        if bank == 0x1e {
            assert_eq!((pending.start, pending.deadline), (450, 495));
            assert_eq!(cpu.cycles(), 30);
        }
        assert_eq!(
            pending.bank_access,
            if bank == 0 {
                Some(BankAccess {
                    address: target,
                    direction: Direction::Write,
                    value,
                })
            } else {
                None
            }
        );
        assert_eq!(pending.writes, WorkWrites::default());
        assert_eq!(pending.psg_writes, PsgWrites::default());
        assert_eq!(pending.opn2_writes, Opn2Writes::default());
        let deadline = pending.deadline;
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(cut..deadline, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        assert!(schedule.pending.is_none());
        if bank == 0x1e {
            assert_eq!(cpu.regs().pc, 8);
            assert_eq!(cpu.regs().ix, 0x8030);
            assert_eq!(cpu.cycles(), 33);
            let mut witness_memory = SoundMemory::default();
            witness_memory.set_reset(false);
            witness_memory.bank = bank;
            witness_memory.ram[..program.len()].copy_from_slice(&program);
            let mut witness_cpu = Cpu::new();
            let mut continuation = BusState::default();
            let mut captured = None;
            for _ in 0..16 {
                let mut candidate = CandidateBus::new(&mut witness_memory, &cartridge);
                witness_cpu
                    .step_bus_segment(&mut candidate, &mut continuation)
                    .unwrap();
                assert_eq!(candidate.fault.get(), None);
                if let Some(access) = candidate.bank_access.get() {
                    captured = Some(access);
                    break;
                }
            }
            assert_eq!(
                captured,
                Some(BankAccess {
                    address: target,
                    direction: Direction::Write,
                    value,
                })
            );
            assert_eq!(witness_cpu.regs().pc, 8);
            assert_eq!(witness_cpu.regs().ix, 0x8030);
            assert_eq!(witness_cpu.cycles(), 33);
        }
        if bank == 0 {
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cartridge,
                &mut [],
                SoundInterval::new(deadline..deadline + 1, &mut audio),
            );
            let pending = schedule.pending.as_ref().unwrap();
            assert_eq!(
                pending.bank_access,
                Some(BankAccess {
                    address: 0x7ffe,
                    direction: Direction::Write,
                    value: 4,
                })
            );
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cartridge,
                &mut [],
                SoundInterval::new(deadline + 1..deadline + 45, &mut audio),
            );
            assert_eq!(schedule.fault, None);
            assert!(schedule.pending.is_none());
            assert_eq!(cpu.regs().sp, 0xfffe);
            assert_eq!(cpu.regs().pc, 0x38);
        }
        assert_eq!(cartridge, before);
        assert_eq!(memory.bank, bank);
    }
}
