use super::*;
use crate::cartridge::ssf2::tests::{access, cartridge, page_program};
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn memory(program: &[u8]) -> SoundMemory {
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.bank = 0x10;
    memory.ram[..program.len()].copy_from_slice(program);
    memory
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn banked_rom_data_replays_after_reservation_and_rejects_control_store() {
    bounded_split_banked_replay();
    for requester in [Requester::MAIN, Requester::VDP] {
        let mut cart = cartridge(&[]);
        cart.ssf2.as_mut().unwrap().control(access(0xa130f3), 8);
        let mut cpu = Cpu::new();
        let mut memory = memory(&[0x3a, 0, 0x80, 0x76]);
        let mut audio = Audio::default();
        let mut schedule = SoundSchedule::default();
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut [],
            SoundInterval::new(0..151, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        assert_eq!(cpu.regs().a, 0);
        assert!(schedule.pending.as_ref().unwrap().replay.is_some());
        let occupied = [Reservation {
            requester,
            ticks: 150..1500,
        }];
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut [],
            SoundInterval {
                ticks: 151..1544,
                occupied: &occupied,
                audio: &mut audio,
                sound_irq: None,
            },
        );
        cart.ssf2.as_mut().unwrap().control(access(0xa130f3), 9);
        memory.bank = 0x1ff;
        let mut writer = StateWriter::new();
        schedule.write_state(&cpu, &mut writer).unwrap();
        let mut restored_cpu = Cpu::new();
        let mut restored = SoundSchedule::default();
        restored
            .read_state(
                &mut restored_cpu,
                &mut StateReader::new(&writer.into_bytes()),
            )
            .unwrap();
        restored.validate_cartridge(&cart).unwrap();
        for missing in 0..3 {
            let mut bad = restored.clone();
            let pending = bad.pending.as_mut().unwrap();
            match missing {
                0 => pending.replay = None,
                1 => pending.bank_access = None,
                _ => pending.observed_bank_access = None,
            }
            assert!(bad.validate_cartridge(&cart).is_err());
        }
        let mut other_memory = memory.clone();
        let mut other_cart = cart.clone();
        let mut other_audio = audio.clone();
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut [],
            SoundInterval::new(1544..1545, &mut audio),
        );
        restored.advance(
            &mut restored_cpu,
            &mut other_memory,
            &mut other_cart,
            &mut [],
            SoundInterval::new(1544..1545, &mut other_audio),
        );
        assert_eq!(schedule.fault, None);
        assert_eq!(cpu.regs().a, 9);
        assert_eq!(memory.bank, 0x10);
        assert_eq!(
            (cpu, memory, cart, audio, schedule),
            (
                restored_cpu,
                other_memory,
                other_cart,
                other_audio,
                restored
            )
        );
    }
    {
        let mut cart = cartridge(&[]);
        cart.ssf2.as_mut().unwrap().control(access(0xa130f3), 8);
        let before = cart.clone();
        let mut memory = memory(&[0xdd, 0x21, 0x30, 0x80, 0xdd, 0x36, 0x3a, 0xff, 0x76]);
        let mut cpu = Cpu::new();
        let mut audio = Audio::default();
        let mut schedule = SoundSchedule::default();
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut [],
            SoundInterval::new(0..451, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        let pending = schedule.pending.as_ref().unwrap();
        assert_eq!((pending.start, pending.deadline), (450, 495));
        assert!(pending.any_bank_access);
        assert!(pending.bank_access.is_none() && pending.replay.is_none());
        assert_eq!(
            pending.observed_bank_access,
            Some(BankAccess {
                address: 0x8006a,
                direction: Direction::Write,
                value: 0xff,
            })
        );
        schedule.validate_cartridge(&cart).unwrap();
        let mut forged = schedule.clone();
        let invalid = forged.pending.as_mut().unwrap();
        invalid.memory.bank = 0x142;
        invalid.observed_bank_access.as_mut().unwrap().address = 0xa130f3;
        assert!(forged.validate_cartridge(&cart).is_err());
        let mut writer = StateWriter::new();
        schedule.write_state(&cpu, &mut writer).unwrap();
        let bytes = writer.into_bytes();
        let mut reader = StateReader::new(&bytes);
        let mut restored_cpu = Cpu::new();
        let mut restored = SoundSchedule::default();
        restored.read_state(&mut restored_cpu, &mut reader).unwrap();
        assert!(reader.is_exhausted());
        restored.validate_cartridge(&cart).unwrap();
        let mut other_memory = memory.clone();
        let mut other_cart = cart.clone();
        let mut other_audio = audio.clone();
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut [],
            SoundInterval::new(451..495, &mut audio),
        );
        restored.advance(
            &mut restored_cpu,
            &mut other_memory,
            &mut other_cart,
            &mut [],
            SoundInterval::new(451..495, &mut other_audio),
        );
        assert_eq!(schedule.fault, None);
        assert!(schedule.pending.is_none());
        assert_eq!(
            (cpu.regs().pc, cpu.regs().ix, cpu.cycles()),
            (8, 0x8030, 33)
        );
        assert_eq!(cart, before);
        assert_eq!(
            (cpu, memory, cart, audio, schedule),
            (
                restored_cpu,
                other_memory,
                other_cart,
                other_audio,
                restored
            )
        );
    }
    let mut cart = cartridge(&[]);
    let before = cart.ssf2.clone();
    let mut memory = memory(&[0x3e, 8, 0x32, 0xf3, 0xb0, 0x76]);
    memory.bank = 0x142;
    let mut schedule = SoundSchedule::default();
    schedule.advance(
        &mut Cpu::new(),
        &mut memory,
        &mut cart,
        &mut [],
        SoundInterval::new(0..1000, &mut Audio::default()),
    );
    assert!(matches!(
        schedule.fault,
        Some(SoundFault::UnsupportedAccess {
            direction: Direction::Write,
            ..
        })
    ));
    assert_eq!(cart.ssf2, before);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn banked_rom_pending_opcode_accepts_same_byte_and_rejects_changed_byte() {
    for changed in [false, true] {
        let mut cart = cartridge(&[]);
        page_program(&mut cart, 8, &[0]);
        page_program(&mut cart, 9, &[if changed { 0x76 } else { 0 }]);
        cart.ssf2.as_mut().unwrap().control(access(0xa130f3), 8);
        let mut memory = memory(&[0xc3, 0, 0x80]);
        let mut cpu = Cpu::new();
        let mut schedule = SoundSchedule::default();
        let mut audio = Audio::default();
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut [],
            SoundInterval::new(0..151, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        assert!(schedule.state.next_segment_reads_opcode());
        let deadline = schedule.pending.as_ref().unwrap().deadline;
        let before = cpu.clone();
        cart.ssf2.as_mut().unwrap().control(access(0xa130f3), 9);
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut [],
            SoundInterval::new(151..deadline, &mut audio),
        );
        if changed {
            assert!(matches!(
                schedule.fault,
                Some(SoundFault::UnstableCartridgeRead { .. })
            ));
            assert_eq!(cpu, before);
        } else {
            assert_eq!(schedule.fault, None);
            assert_ne!(cpu, before);
        }
    }
}

fn bounded_split_banked_replay() {
    for product in [b"GM T-172116-00", b"GM T-172026-04"] {
        let mut cart = crate::cartridge::ram::profile_tests::bounded_split_fixture(product);
        cart.save.as_mut().unwrap().bytes[0] = 0x35;
        let mut cpu = Cpu::new();
        let mut memory = memory(&[0x3a, 1, 0x80, 0x76]);
        memory.bank = 0x40;
        let mut audio = Audio::default();
        let mut schedule = SoundSchedule::default();
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut [],
            SoundInterval::new(0..151, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        let pending = schedule.pending.as_ref().unwrap();
        assert_eq!(pending.bank_access.unwrap().address, 0x200001);
        assert_eq!(pending.observed_bank_access, pending.bank_access);
        assert!(pending.replay.is_some() && pending.reservation().is_some());
        schedule.validate_cartridge(&cart).unwrap();
        for field in 0..3 {
            for address in [0x210000, 0x2fffff, 0x200001] {
                for direction in [Direction::Read, Direction::Write] {
                    if address == 0x200001 && direction == Direction::Read {
                        continue;
                    }
                    let mut forged = schedule.clone();
                    let bad = forged.pending.as_mut().unwrap();
                    match field {
                        0 => {
                            let access = bad.bank_access.as_mut().unwrap();
                            access.address = address;
                            access.direction = direction;
                        }
                        1 => {
                            let access = bad.observed_bank_access.as_mut().unwrap();
                            access.address = address;
                            access.direction = direction;
                        }
                        _ => {
                            bad.replay.as_mut().unwrap().bank_identity = Some((address, direction))
                        }
                    }
                    assert!(forged.validate_cartridge(&cart).is_err());
                }
            }
        }
        for missing in 0..3 {
            let mut forged = schedule.clone();
            let bad = forged.pending.as_mut().unwrap();
            match missing {
                0 => bad.bank_access = None,
                1 => bad.replay = None,
                _ => bad.observed_bank_access = None,
            }
            assert!(forged.validate_cartridge(&cart).is_err());
        }
        let mut writer = StateWriter::new();
        schedule.write_state(&cpu, &mut writer).unwrap();
        let encoded = writer.into_bytes();
        let mut restored = SoundSchedule::default();
        let mut restored_cpu = Cpu::new();
        let mut reader = StateReader::new(&encoded);
        restored.read_state(&mut restored_cpu, &mut reader).unwrap();
        assert!(reader.is_exhausted());
        restored.validate_cartridge(&cart).unwrap();
        cart.save.as_mut().unwrap().bytes[0] = 0xa6;
        memory.bank = 0;
        let mut restored_memory = memory.clone();
        let mut restored_audio = audio.clone();
        let mut restored_cart = cart.clone();
        for (cpu, memory, cart, schedule, audio) in [
            (&mut cpu, &mut memory, &mut cart, &mut schedule, &mut audio),
            (
                &mut restored_cpu,
                &mut restored_memory,
                &mut restored_cart,
                &mut restored,
                &mut restored_audio,
            ),
        ] {
            schedule.advance(
                cpu,
                memory,
                cart,
                &mut [],
                SoundInterval::new(151..195, audio),
            );
            assert_eq!(schedule.fault, None);
            assert_eq!(cpu.regs().a, 0xa6);
            assert_eq!(memory.bank, 0x40);
        }
        assert_eq!(
            (cpu, memory, cart, schedule, audio),
            (
                restored_cpu,
                restored_memory,
                restored_cart,
                restored,
                restored_audio
            )
        );
    }
}

impl SoundSchedule {
    pub(crate) fn assert_extended_windows(cartridge: &Cartridge) {
        use zeff_z80::Z80Bus;
        for requester in [Requester::MAIN, Requester::VDP] {
            let mut cart = cartridge.clone();
            cart.extended_ssf.as_mut().unwrap().power_on();
            let mut memory = SoundMemory::default();
            memory.set_reset(false);
            memory.ram[..4].copy_from_slice(&[0x3a, 0, 0x90, 0x76]);
            let mut cpu = Cpu::new();
            let mut audio = Audio::default();
            let mut schedule = SoundSchedule::default();
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cart,
                &mut [],
                SoundInterval::new(0..151, &mut audio),
            );
            assert_eq!(schedule.fault, None);
            assert!(schedule.pending.as_ref().unwrap().replay.is_some());
            assert_eq!(
                schedule
                    .pending
                    .as_ref()
                    .unwrap()
                    .reservation()
                    .unwrap()
                    .requester,
                Requester::SOUND
            );
            let occupied = [Reservation {
                requester,
                ticks: 150..1500,
            }];
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cart,
                &mut [],
                SoundInterval {
                    ticks: 151..1544,
                    occupied: &occupied,
                    audio: &mut audio,
                    sound_irq: None,
                },
            );
            assert_eq!(schedule.fault, None);
            assert_eq!(cpu.regs().a, 0);
            cart.extended_ssf.as_mut().unwrap().control(0xa130f0, 37);
            let mut writer = StateWriter::new();
            schedule.write_state(&cpu, &mut writer).unwrap();
            let bytes = writer.into_bytes();
            let mut other_cpu = Cpu::new();
            let mut other = SoundSchedule::default();
            other
                .read_state(&mut other_cpu, &mut StateReader::new(&bytes))
                .unwrap();
            other.validate_cartridge(&cart).unwrap();
            for missing in 0..3 {
                let mut bad = other.clone();
                let p = bad.pending.as_mut().unwrap();
                match missing {
                    0 => p.replay = None,
                    1 => p.bank_access = None,
                    _ => p.observed_bank_access = None,
                }
                assert!(bad.validate_cartridge(&cart).is_err());
            }
            let mut other_cart = cart.clone();
            let mut other_memory = memory.clone();
            let mut other_audio = audio.clone();
            for (cpu, memory, cart, schedule, audio) in [
                (&mut cpu, &mut memory, &mut cart, &mut schedule, &mut audio),
                (
                    &mut other_cpu,
                    &mut other_memory,
                    &mut other_cart,
                    &mut other,
                    &mut other_audio,
                ),
            ] {
                schedule.advance(
                    cpu,
                    memory,
                    cart,
                    &mut [],
                    SoundInterval::new(1544..1545, audio),
                );
                assert_eq!(schedule.fault, None);
                assert_eq!(cpu.regs().a, 37);
            }
            assert_eq!(
                (cpu, memory, cart, schedule, audio),
                (other_cpu, other_memory, other_cart, other, other_audio)
            );
        }
        for changed in [false, true] {
            let mut cart = cartridge.clone();
            cart.extended_ssf.as_mut().unwrap().power_on();
            let rom = std::sync::Arc::make_mut(&mut cart.bytes);
            rom[0x1000] = 0;
            rom[37 * 0x80000 + 0x1000] = if changed { 0x76 } else { 0 };
            let mut memory = SoundMemory::default();
            memory.set_reset(false);
            memory.ram[..3].copy_from_slice(&[0xc3, 0, 0x90]);
            let mut cpu = Cpu::new();
            let mut audio = Audio::default();
            let mut schedule = SoundSchedule::default();
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cart,
                &mut [],
                SoundInterval::new(0..151, &mut audio),
            );
            assert_eq!(schedule.fault, None);
            assert!(schedule.state.next_segment_reads_opcode());
            let deadline = schedule.pending.as_ref().unwrap().deadline;
            let before = cpu.clone();
            cart.extended_ssf.as_mut().unwrap().control(0xa130f0, 37);
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cart,
                &mut [],
                SoundInterval::new(151..deadline, &mut audio),
            );
            if changed {
                assert!(matches!(
                    schedule.fault,
                    Some(SoundFault::UnstableCartridgeRead { .. })
                ));
                assert_eq!(cpu, before);
            } else {
                assert_eq!(schedule.fault, None);
            }
        }
        for (bank, address, write) in [
            (0, 0x9000, true),
            (0x142, 0xb0f0, true),
            (0x80, 0x8001, false),
        ] {
            let mut memory = SoundMemory {
                bank,
                ..Default::default()
            };
            let mut bus = CandidateBus::new(&mut memory, cartridge);
            if write {
                bus.cpu_write(address, 0x25);
            } else {
                bus.cpu_read(address);
            }
            assert!(bus.fault.get().is_some());
            assert!(bus.sf001_writes.is_empty());
            assert!(bus.sf002_writes.is_empty());
        }
    }
}
