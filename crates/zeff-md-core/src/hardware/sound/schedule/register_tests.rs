use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_z80::Z80Bus;

fn board() -> Cartridge {
    Cartridge::new(crate::cartridge::registers::tests::rom(0)).unwrap()
}

fn memory(program: &[u8]) -> SoundMemory {
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.bank = 0x80;
    memory.ram[..program.len()].copy_from_slice(program);
    memory
}

fn value(cartridge: &Cartridge, address: u32) -> u8 {
    cartridge.registers.as_ref().unwrap().read(address).unwrap()
}

fn write(cartridge: &mut Cartridge, index: u8, value: u8) {
    cartridge.registers.as_mut().unwrap().write(index, value);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn register_write_fault_path_publishes_once_at_reserved_deadline_with_resolved_bank() {
    for requester in [Requester::MAIN, Requester::VDP] {
        let mut cartridge = board();
        let mut memory = memory(&[0x3e, 0x42, 0x32, 4, 0x80, 0x76]);
        let mut cpu = Cpu::new();
        let mut audio = Audio::default();
        let mut schedule = SoundSchedule::default();
        let mut ram = vec![0xde; WORK_RAM_SIZE];
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut ram,
            SoundInterval::new(0..256, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        let pending = schedule.pending.as_ref().unwrap();
        assert_eq!((pending.start, pending.deadline), (255, 300));
        assert_eq!(
            pending.bank_access,
            Some(BankAccess {
                address: 0x400004,
                direction: Direction::Write,
                value: 0x42
            })
        );
        assert_eq!(value(&cartridge, 0x400004), 0);
        assert_eq!(pending.writes, WorkWrites::default());
        assert_eq!(pending.psg_writes, PsgWrites::default());
        assert_eq!(pending.opn2_writes, Opn2Writes::default());
        memory.bank = 0x1ff;
        write(&mut cartridge, 0, 0xa6);
        let mut writer = StateWriter::new();
        schedule.write_state(&cpu, &mut writer).unwrap();
        let encoded = writer.into_bytes();
        let mut restored = SoundSchedule::default();
        let mut restored_cpu = Cpu::new();
        restored
            .read_state(&mut restored_cpu, &mut StateReader::new(&encoded))
            .unwrap();
        let mut other = (
            restored_cpu,
            memory.clone(),
            cartridge.clone(),
            ram.clone(),
            audio.clone(),
            restored,
        );
        let occupied = [Reservation {
            requester,
            ticks: 255..1500,
        }];
        for (cpu, memory, cartridge, ram, audio, schedule) in [
            (
                &mut cpu,
                &mut memory,
                &mut cartridge,
                &mut ram,
                &mut audio,
                &mut schedule,
            ),
            (
                &mut other.0,
                &mut other.1,
                &mut other.2,
                &mut other.3,
                &mut other.4,
                &mut other.5,
            ),
        ] {
            schedule.advance(
                cpu,
                memory,
                cartridge,
                ram,
                SoundInterval {
                    ticks: 256..1544,
                    occupied: &occupied,
                    audio,
                    sound_irq: None,
                },
            );
            assert_eq!(schedule.fault, None);
            assert_eq!(schedule.pending.as_ref().unwrap().deadline, 1545);
            assert_eq!(value(cartridge, 0x400004), 0);
            schedule.advance(
                cpu,
                memory,
                cartridge,
                ram,
                SoundInterval::new(1544..1545, audio),
            );
            assert_eq!(schedule.fault, None);
            assert!(schedule.pending.is_none());
            assert_eq!(value(cartridge, 0x400004), 0x42);
            assert_eq!(value(cartridge, 0x400000), 0xa6);
            assert!(ram.iter().all(|&byte| byte == 0xde));
        }
        assert_eq!((cpu, memory, cartridge, ram, audio, schedule), other);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn register_read_samples_live_latch_after_arbitration_and_restores_captured_bank() {
    super::or_bank_tests::assert_projection_and_replay();
    inserted_save_publication();
    inserted_save_fetches();
    for requester in [Requester::MAIN, Requester::VDP] {
        for address in [0, 2, 4, 6] {
            let mut cartridge = board();
            write(&mut cartridge, u8::from(address >= 4), 0x37);
            let mut memory = memory(&[0x3a, address, 0x80, 0x76]);
            let mut cpu = Cpu::new();
            let mut audio = Audio::default();
            let mut schedule = SoundSchedule::default();
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cartridge,
                &mut [],
                SoundInterval::new(0..151, &mut audio),
            );
            assert_eq!(schedule.fault, None);
            let pending = schedule.pending.as_ref().unwrap();
            assert_eq!(pending.deadline, 195);
            assert!(pending.replay.is_some());
            assert_eq!(cpu.regs().a, 0);
            write(&mut cartridge, u8::from(address >= 4), 0x42);
            memory.bank = 0x1ff;
            memory.ram[0x777] = 0xa6;
            let mut copy = (
                cpu.clone(),
                memory.clone(),
                cartridge.clone(),
                audio.clone(),
                schedule.clone(),
            );
            let occupied = [Reservation {
                requester,
                ticks: 150..1500,
            }];
            for (cpu, memory, cartridge, audio, schedule) in [
                (
                    &mut cpu,
                    &mut memory,
                    &mut cartridge,
                    &mut audio,
                    &mut schedule,
                ),
                (
                    &mut copy.0,
                    &mut copy.1,
                    &mut copy.2,
                    &mut copy.3,
                    &mut copy.4,
                ),
            ] {
                schedule.advance(
                    cpu,
                    memory,
                    cartridge,
                    &mut [],
                    SoundInterval {
                        ticks: 151..1544,
                        occupied: &occupied,
                        audio,
                        sound_irq: None,
                    },
                );
                assert_eq!(cpu.regs().a, 0);
                assert_eq!(schedule.pending.as_ref().unwrap().deadline, 1545);
                write(cartridge, u8::from(address >= 4), 0xff);
                schedule.advance(
                    cpu,
                    memory,
                    cartridge,
                    &mut [],
                    SoundInterval::new(1544..1545, audio),
                );
                assert_eq!(schedule.fault, None);
                assert_eq!(cpu.regs().a, 0xff);
                assert_eq!(memory.bank, 0x80);
                assert_eq!(memory.ram[0x777], 0xa6);
            }
            assert_eq!((cpu, memory, cartridge, audio, schedule), copy);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn admitted_register_read_then_write_replays_deadline_and_rmw_guard_preserves_state() {
    let mut cartridge = board();
    write(&mut cartridge, 0, 0xa6);
    write(&mut cartridge, 1, 0x37);
    let mut memory = memory(&[0x21, 4, 0x80, 0x7e, 0x3c, 0x77, 0x76]);
    let mut cpu = Cpu::new();
    let mut audio = Audio::default();
    let mut schedule = SoundSchedule::default();
    let mut now = 0;
    loop {
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(now..now + 1, &mut audio),
        );
        now += 1;
        assert_eq!(schedule.fault, None);
        assert!(now < 500);
        if schedule
            .pending
            .as_ref()
            .is_some_and(|pending| pending.replay.is_some())
        {
            break;
        }
    }
    let read_deadline = schedule.pending.as_ref().unwrap().deadline;
    assert_eq!(read_deadline, 255);
    write(&mut cartridge, 1, 0x42);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(now..read_deadline, &mut audio),
    );
    assert_eq!(cpu.regs().a, 0x42);
    write(&mut cartridge, 1, 0x17);
    now = read_deadline;
    loop {
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(now..now + 1, &mut audio),
        );
        now += 1;
        assert_eq!(schedule.fault, None);
        assert!(now < 500);
        if schedule
            .pending
            .as_ref()
            .is_some_and(|pending| pending.register_writes != RegisterWrites::default())
        {
            break;
        }
    }
    assert_eq!(value(&cartridge, 0x400004), 0x17);
    let deadline = schedule.pending.as_ref().unwrap().deadline;
    assert_eq!(deadline, 420);
    let mut reset = (
        cpu.clone(),
        memory.clone(),
        cartridge.clone(),
        audio.clone(),
        schedule.clone(),
    );
    reset.4.reset_cpu(
        &mut reset.0,
        &mut reset.1,
        &mut crate::sound_trace::NoSoundCapture,
    );
    reset.1.set_reset(true);
    reset.4.advance(
        &mut reset.0,
        &mut reset.1,
        &mut reset.2,
        &mut [],
        SoundInterval::new(now..deadline, &mut reset.3),
    );
    assert_eq!(value(&reset.2, 0x400000), 0xa6);
    assert_eq!(value(&reset.2, 0x400004), 0x17);
    assert!(reset.4.pending.is_none());
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(now..deadline, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(value(&cartridge, 0x400004), 0x43);
    assert_eq!(value(&cartridge, 0x400000), 0xa6);
    assert_eq!(
        (cpu.regs().a, cpu.regs().hl(), cpu.regs().pc),
        (0x43, 0x8004, 6)
    );

    let mut negative_cpu = Cpu::new();
    let mut negative_memory = self::memory(&[0x21, 4, 0x80, 0x34, 0x76]);
    let mut negative_audio = Audio::default();
    let mut negative_schedule = SoundSchedule::default();
    let mut negative_ram = vec![0xde; WORK_RAM_SIZE];
    let mut rejected = false;
    for at in 0..500 {
        let before = (
            negative_cpu.clone(),
            negative_memory.clone(),
            cartridge.clone(),
            negative_ram.clone(),
        );
        let continuation = negative_schedule.state;
        negative_schedule.advance(
            &mut negative_cpu,
            &mut negative_memory,
            &mut cartridge,
            &mut negative_ram,
            SoundInterval::new(at..at + 1, &mut negative_audio),
        );
        if let Some(fault) = negative_schedule.fault {
            assert_eq!(at, 210);
            assert_eq!(
                fault,
                SoundFault::UnsupportedAccess {
                    address: 0x8004,
                    direction: Direction::Read
                }
            );
            assert_eq!(
                (
                    negative_cpu.clone(),
                    negative_memory.clone(),
                    cartridge.clone(),
                    negative_ram.clone()
                ),
                before
            );
            assert_eq!(negative_schedule.state, continuation);
            assert!(negative_schedule.pending.is_none());
            assert_eq!(negative_cpu.cycles(), 14);
            assert_eq!(
                (negative_cpu.regs().hl(), negative_cpu.regs().pc),
                (0x8004, 4)
            );
            rejected = true;
            break;
        }
    }
    assert!(rejected);
    let before = (
        negative_cpu.clone(),
        negative_memory.clone(),
        cartridge.clone(),
        negative_ram.clone(),
        negative_schedule.clone(),
    );
    negative_schedule.advance(
        &mut negative_cpu,
        &mut negative_memory,
        &mut cartridge,
        &mut negative_ram,
        SoundInterval::new(211..500, &mut negative_audio),
    );
    assert_eq!(
        (
            negative_cpu,
            negative_memory,
            cartridge.clone(),
            negative_ram,
            negative_schedule
        ),
        before
    );
    assert_eq!(value(&cartridge, 0x400000), 0xa6);
    assert_eq!(value(&cartridge, 0x400004), 0x43);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn stopped_mutable_reads_unmatched_reads_overflow_and_ordinary_boards_keep_guards() {
    let mut cartridge = board();
    let mut memory = memory(&[0x3a, 4, 0x80, 0x76]);
    let cpu = Cpu::new();
    let before = (cartridge.clone(), memory.clone(), cpu.clone());
    assert_eq!(
        PendingSound::prepare(&cpu, &memory, &cartridge, 0).unwrap_err(),
        SoundFault::UnsupportedAccess {
            address: 0x8004,
            direction: Direction::Read
        }
    );
    assert_eq!((cartridge.clone(), memory.clone(), cpu.clone()), before);
    for address in [0x8001, 0x8003, 0x8005, 0x8007, 0x8008, 0xffff] {
        let candidate = CandidateBus::new(&mut memory, &cartridge);
        candidate.cpu_read(address);
        assert_eq!(
            candidate.fault.get(),
            Some(SoundFault::UnsupportedAccess {
                address,
                direction: Direction::Read
            })
        );
    }
    let mut candidate = CandidateBus::new(&mut memory, &cartridge);
    for (address, value) in [(0x8000, 0x42), (0x8004, 0xff)] {
        candidate.cpu_write(address, value);
    }
    assert_eq!(candidate.fault.get(), None);
    candidate.cpu_write(0x8002, 0x17);
    assert_eq!(
        candidate.fault.get(),
        Some(SoundFault::UnsupportedAccess {
            address: 0x8002,
            direction: Direction::Write
        })
    );
    assert_eq!(candidate.writes, WorkWrites::default());
    assert_eq!(candidate.psg_writes, PsgWrites::default());
    assert_eq!(candidate.opn2_writes, Opn2Writes::default());
    assert_eq!(value(&cartridge, 0x400000), 0);
    assert_eq!(value(&cartridge, 0x400004), 0);
    let journal = candidate.register_writes;
    journal.publish(&mut cartridge);
    assert_eq!(value(&cartridge, 0x400000), 0x42);
    assert_eq!(value(&cartridge, 0x400004), 0xff);
    let mut bytes = crate::cartridge::registers::tests::rom(0);
    bytes[0x18e] ^= 1;
    let ordinary = Cartridge::new(bytes).unwrap();
    let mut candidate = CandidateBus::new(&mut memory, &ordinary);
    candidate.cpu_write(0x8004, 0x42);
    assert_eq!(
        candidate.fault.get(),
        Some(SoundFault::UnsupportedAccess {
            address: 0x8004,
            direction: Direction::Write
        })
    );
    assert_eq!(candidate.register_writes, RegisterWrites::default());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn register_replay_instability_and_bus_grant_preserve_transaction_boundaries() {
    let mut cartridge = board();
    let mut memory = memory(&[0x3a, 4, 0x80, 0x76]);
    let mut cpu = Cpu::new();
    let mut audio = Audio::default();
    let mut schedule = SoundSchedule::default();
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(0..151, &mut audio),
    );
    let mut pending = schedule.pending.take().unwrap();
    pending.replay.as_mut().unwrap().bank_identity = Some((0x400002, Direction::Read));
    let before = pending.clone();
    assert_eq!(
        pending.refresh_reads(&cpu, &memory, &cartridge, &audio),
        Err(SoundFault::UnstableCartridgeRead { pc: 0 })
    );
    assert_eq!(pending, before);
    let mut memory = self::memory(&[0x3e, 0x42, 0x32, 4, 0x80, 0x76]);
    let mut cpu = Cpu::new();
    let mut audio = Audio::default();
    let mut schedule = SoundSchedule::default();
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(0..256, &mut audio),
    );
    memory.request(true);
    assert!(!memory.granted);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(256..299, &mut audio),
    );
    assert!(!memory.granted);
    assert_eq!(value(&cartridge, 0x400004), 0);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(299..300, &mut audio),
    );
    assert_eq!(value(&cartridge, 0x400004), 0x42);
    assert!(memory.granted);
    assert!(schedule.pending.is_none());
}

fn inserted_save_board() -> Cartridge {
    use crate::cartridge::registers::{LockOn, LockOnSave};
    let mut cartridge = Cartridge::new(crate::cartridge::registers::tests::normal_rom_fixture(
        LockOn::TwoMiB,
    ))
    .unwrap();
    cartridge.lock_on = Some(LockOn::TwoMiB);
    cartridge.lock_on_save = Some(LockOnSave::new());
    cartridge
}

fn restore_inserted(
    cpu: &Cpu,
    schedule: &SoundSchedule,
    cartridge: &Cartridge,
) -> (Cpu, SoundSchedule) {
    let mut writer = StateWriter::new();
    schedule.write_state(cpu, &mut writer).unwrap();
    let mut restored = SoundSchedule::default();
    let mut restored_cpu = Cpu::new();
    restored
        .read_state(
            &mut restored_cpu,
            &mut StateReader::new(&writer.into_bytes()),
        )
        .unwrap();
    restored.validate_cartridge(cartridge).unwrap();
    (restored_cpu, restored)
}

fn inserted_save_publication() {
    for change in 0..3 {
        for target in [
            0x200001, 0x200101, 0x2003ff, 0x200000, 0x2003fe, 0x300000, 0x3fffff, 0x202000,
        ] {
            let odd = target < 0x300000 && target & 1 != 0;
            let sensitive = target != 0x202000;
            if change != 0 && sensitive && !odd {
                continue;
            }
            for requester in [Requester::MAIN, Requester::VDP] {
                let mut cartridge = inserted_save_board();
                let save = cartridge.lock_on_save.as_mut().unwrap();
                save.enabled = change != 0;
                save.load(&[0x37; 512]).unwrap();
                let address = 0x8000 | (target & 0x7fff) as u16;
                let [low, high] = address.to_le_bytes();
                let mut memory = memory(&[0x3a, low, high, 0x76]);
                memory.bank = (target >> 15) as u16;
                let mut cpu = Cpu::new();
                let mut audio = Audio::default();
                let mut schedule = SoundSchedule::default();
                schedule.advance(
                    &mut cpu,
                    &mut memory,
                    &mut cartridge,
                    &mut [],
                    SoundInterval::new(0..151, &mut audio),
                );
                assert_eq!(schedule.fault, None);
                assert_eq!(
                    schedule.pending.as_ref().unwrap().replay.is_some(),
                    sensitive
                );
                schedule.validate_cartridge(&cartridge).unwrap();
                if sensitive {
                    for mutation in 0..7 {
                        let mut invalid = schedule.clone();
                        let pending = invalid.pending.as_mut().unwrap();
                        match mutation {
                            0 => pending.replay = None,
                            1 => pending.bank_access = None,
                            2 => pending.replay.as_mut().unwrap().bank = 0,
                            3 => {
                                pending.replay = None;
                                pending.bank_access = None;
                            }
                            4 => pending.observed_bank_access = None,
                            5 => pending.observed_bank_access.as_mut().unwrap().address ^= 0x2000,
                            _ => {
                                pending.observed_bank_access.as_mut().unwrap().direction =
                                    Direction::Write;
                                pending.bank_access.as_mut().unwrap().direction = Direction::Write;
                                pending.replay.as_mut().unwrap().bank_identity =
                                    Some((target, Direction::Write));
                            }
                        }

                        let mut writer = StateWriter::new();
                        invalid.write_state(&cpu, &mut writer).unwrap();
                        let mut decoded = SoundSchedule::default();
                        let mut decoded_cpu = Cpu::new();
                        decoded
                            .read_state(
                                &mut decoded_cpu,
                                &mut StateReader::new(&writer.into_bytes()),
                            )
                            .unwrap();
                        assert!(decoded.validate_cartridge(&cartridge).is_err());
                    }
                }
                let (restored_cpu, restored) = restore_inserted(&cpu, &schedule, &cartridge);
                let save = cartridge.lock_on_save.as_mut().unwrap();
                save.enabled = change != 1;
                save.load(&[0x42; 512]).unwrap();
                memory.bank = 0x1ff;
                memory.ram[0x777] = 0xa6;
                let mut other = (
                    restored_cpu,
                    memory.clone(),
                    cartridge.clone(),
                    audio.clone(),
                    restored,
                );
                let occupied = [Reservation {
                    requester,
                    ticks: 150..1500,
                }];
                for (cpu, memory, cartridge, audio, schedule) in [
                    (
                        &mut cpu,
                        &mut memory,
                        &mut cartridge,
                        &mut audio,
                        &mut schedule,
                    ),
                    (
                        &mut other.0,
                        &mut other.1,
                        &mut other.2,
                        &mut other.3,
                        &mut other.4,
                    ),
                ] {
                    let before = (
                        cpu.clone(),
                        memory.clone(),
                        cartridge.clone(),
                        schedule.state,
                    );
                    schedule.advance(
                        cpu,
                        memory,
                        cartridge,
                        &mut [],
                        SoundInterval {
                            ticks: 151..1544,
                            occupied: &occupied,
                            sound_irq: None,
                            audio,
                        },
                    );
                    assert_eq!(schedule.fault, None);
                    assert_eq!(schedule.pending.as_ref().unwrap().deadline, 1545);
                    assert_eq!(
                        (
                            cpu.clone(),
                            memory.clone(),
                            cartridge.clone(),
                            schedule.state
                        ),
                        before
                    );
                    schedule.advance(
                        cpu,
                        memory,
                        cartridge,
                        &mut [],
                        SoundInterval::new(1544..1545, audio),
                    );
                    if sensitive && !odd {
                        assert_eq!(
                            schedule.fault,
                            Some(SoundFault::UnsupportedAccess {
                                address,
                                direction: Direction::Read
                            })
                        );
                        assert_eq!(
                            (
                                cpu.clone(),
                                memory.clone(),
                                cartridge.clone(),
                                schedule.state
                            ),
                            before
                        );
                    } else {
                        assert_eq!(schedule.fault, None);
                        assert_eq!(
                            cpu.regs().a,
                            if !sensitive || change == 1 {
                                0x28
                            } else {
                                0x42
                            }
                        );
                        assert_eq!(memory.bank, (target >> 15) as u16);
                        assert_eq!(memory.ram[0x777], if sensitive { 0xa6 } else { 0 });
                    }
                    assert!(schedule.pending.is_none());
                    assert_eq!(
                        cartridge.lock_on_save.as_ref().unwrap().bytes(),
                        &[0x42; 512]
                    );
                }
                assert_eq!((cpu, memory, cartridge, audio, schedule), other);
            }
        }
    }
}

fn inserted_save_fetches() {
    use zeff_emu_common::audio_trace::MdTraceSource;
    for opcode in [0, 0xcb, 0xed, 0xdd, 0xfd] {
        for enabled in [false, true] {
            for changed in [false, true] {
                for outside in [false, true] {
                    if outside && (opcode != 0 || changed) {
                        continue;
                    }
                    for requester in [Requester::MAIN, Requester::VDP] {
                        let target = if outside { 0x202000 } else { 0x200001 };
                        let address = 0x8000 | (target & 0x7fff) as u16;
                        let mut cartridge = inserted_save_board();
                        std::sync::Arc::make_mut(&mut cartridge.bytes)[target as usize] =
                            opcode ^ u8::from(changed && enabled);
                        let save = cartridge.lock_on_save.as_mut().unwrap();
                        save.enabled = enabled;
                        assert!(save.write(0x200001, opcode ^ u8::from(changed && !enabled)));
                        let mut memory = memory(&[]);
                        memory.bank = 0x40;
                        let mut cpu =
                            Cpu::new_with_reset(zeff_z80::ResetState::new(address, 0x1ffe));
                        let mut audio = Audio::default();
                        let mut schedule = SoundSchedule::default();
                        schedule.advance(
                            &mut cpu,
                            &mut memory,
                            &mut cartridge,
                            &mut [],
                            SoundInterval::new(0..1, &mut audio),
                        );
                        assert_eq!(schedule.fault, None);
                        let pending = schedule.pending.as_ref().unwrap();
                        assert_eq!(pending.replay.is_some(), !outside);
                        assert_eq!(pending.bank_access.is_some(), !outside);
                        if !outside {
                            let mut forged = schedule.clone();
                            let pending = forged.pending.as_mut().unwrap();
                            pending.bank_access = None;
                            pending.replay = None;
                            pending.observed_bank_access.as_mut().unwrap().address = 0x202000;
                            assert!(forged.validate_cartridge(&cartridge).is_err());
                        }
                        schedule.validate_cartridge(&cartridge).unwrap();
                        let (restored_cpu, restored) =
                            restore_inserted(&cpu, &schedule, &cartridge);
                        let mut probe = schedule.pending.as_ref().unwrap().clone();
                        probe.trace_origin = Some(crate::hardware::audio::trace::sound_origin(
                            address, 0x40, &cartridge,
                        ));
                        cartridge.lock_on_save.as_mut().unwrap().enabled = !enabled;
                        memory.bank = 0x1ff;
                        if !changed && !outside {
                            probe
                                .refresh_reads(&cpu, &memory, &cartridge, &audio)
                                .unwrap();
                            assert_eq!(
                                probe.trace_origin.unwrap().source,
                                if enabled {
                                    MdTraceSource::EffectiveRom { offset: target }
                                } else {
                                    MdTraceSource::Unknown
                                }
                            );
                        }
                        let mut other = (
                            restored_cpu,
                            memory.clone(),
                            cartridge.clone(),
                            audio.clone(),
                            restored,
                        );
                        let occupied = [Reservation {
                            requester: if outside && requester == Requester::MAIN {
                                Requester::MAIN_RMW
                            } else {
                                requester
                            },
                            ticks: 0..1000,
                        }];
                        for (cpu, memory, cartridge, audio, schedule) in [
                            (
                                &mut cpu,
                                &mut memory,
                                &mut cartridge,
                                &mut audio,
                                &mut schedule,
                            ),
                            (
                                &mut other.0,
                                &mut other.1,
                                &mut other.2,
                                &mut other.3,
                                &mut other.4,
                            ),
                        ] {
                            let before = (
                                cpu.clone(),
                                memory.clone(),
                                cartridge.clone(),
                                schedule.state,
                            );
                            schedule.advance(
                                cpu,
                                memory,
                                cartridge,
                                &mut [],
                                SoundInterval {
                                    ticks: 1..1001,
                                    occupied: &occupied,
                                    sound_irq: None,
                                    audio,
                                },
                            );
                            assert_eq!(schedule.fault, None);
                            let deadline = schedule.pending.as_ref().unwrap().deadline;
                            assert!(deadline > 1001);
                            schedule.advance(
                                cpu,
                                memory,
                                cartridge,
                                &mut [],
                                SoundInterval::new(1001..deadline, audio),
                            );
                            if changed {
                                assert_eq!(
                                    schedule.fault,
                                    Some(SoundFault::UnstableCartridgeRead { pc: address })
                                );
                                assert_eq!(
                                    (
                                        cpu.clone(),
                                        memory.clone(),
                                        cartridge.clone(),
                                        schedule.state
                                    ),
                                    before
                                );
                            } else {
                                assert_eq!(schedule.fault, None);
                                assert_eq!(cpu.regs().pc, address + 1);
                                assert_eq!(memory.bank, 0x40);
                            }
                            assert!(schedule.pending.is_none());
                        }
                        assert_eq!((cpu, memory, cartridge, audio, schedule), other);
                    }
                }
            }
        }
    }
}
