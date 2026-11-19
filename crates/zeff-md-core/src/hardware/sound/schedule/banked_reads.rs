use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[path = "bounded_odd_cases.rs"]
mod bounded_odd_cases;
#[path = "bounded_switched_cases.rs"]
mod bounded_switched_cases;

fn cartridge(size: usize) -> Cartridge {
    let mut bytes = vec![0x69; 0x300000];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x180..0x18e].copy_from_slice(if size == 8192 {
        b"GM G-5524  -00"
    } else {
        b"GM MK-1239 -00"
    });
    bytes[0x1b0..0x1ba].copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20]);
    bytes[0x1ba..0x1bc].copy_from_slice(&((size * 2 - 1) as u16).to_be_bytes());
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
fn cartridge_read_replays_mapping_data_and_live_ram_with_sampled_bank_after_waits_and_restore() {
    bounded_odd_cases::contract();
    bounded_switched_cases::contract();
    lock_on_publication_cases();
    lock_on_fetch_cases();
    for size in [8192, 32768] {
        for target in [0x200000, 0x200001, 0x206633, 0x3fffff] {
            for indirect in [false, true] {
                for change in 0..3 {
                    for requester in [Requester::MAIN, Requester::VDP] {
                        let mut cartridge = cartridge(size);
                        let save = cartridge.save.as_mut().unwrap();
                        save.enabled = change != 0;
                        save.bytes.fill(0x37);
                        let (mut cpu, mut memory, mut schedule, mut audio, cut) =
                            setup(target, indirect);
                        schedule.advance(
                            &mut cpu,
                            &mut memory,
                            &mut cartridge,
                            &mut [],
                            SoundInterval::new(0..cut, &mut audio),
                        );
                        assert_eq!(schedule.fault, None);
                        let pending = schedule.pending.as_ref().unwrap();
                        assert_eq!(pending.instruction.cycles, 3);
                        assert_eq!(pending.replay.unwrap().accesses, 1);
                        assert_eq!(pending.bank_access.unwrap().address, target);
                        assert_eq!(pending.reservation().unwrap().requester, Requester::SOUND);
                        assert_eq!(cpu.regs().a, 0);
                        let mut writer = StateWriter::new();
                        schedule.write_state(&cpu, &mut writer).unwrap();
                        let encoded = writer.into_bytes();
                        let mut restored = SoundSchedule::default();
                        let mut restored_cpu = Cpu::new();
                        restored
                            .read_state(&mut restored_cpu, &mut StateReader::new(&encoded))
                            .unwrap();
                        assert_eq!(schedule, restored);
                        assert_eq!(cpu, restored_cpu);
                        let save = cartridge.save.as_mut().unwrap();
                        save.enabled = change != 1;
                        if change == 2 {
                            save.bytes.fill(0x42);
                        }
                        memory.bank = 0;
                        memory.ram[0x777] = 0xa6;
                        let cells = save.bytes.clone();
                        let mut restored_memory = memory.clone();
                        let mut restored_audio = audio.clone();
                        let occupied = [Reservation {
                            requester,
                            ticks: (cut - 1)..1500,
                        }];
                        for (source_cpu, source_memory, source_schedule, source_audio) in [
                            (&mut cpu, &mut memory, &mut schedule, &mut audio),
                            (
                                &mut restored_cpu,
                                &mut restored_memory,
                                &mut restored,
                                &mut restored_audio,
                            ),
                        ] {
                            source_schedule.advance(
                                source_cpu,
                                source_memory,
                                &mut cartridge,
                                &mut [],
                                SoundInterval {
                                    ticks: cut..1544,
                                    occupied: &occupied,
                                    sound_irq: None,
                                    audio: source_audio,
                                },
                            );
                            assert_eq!(source_schedule.fault, None);
                            assert_eq!(source_cpu.regs().a, 0);
                            assert_eq!(source_schedule.pending.as_ref().unwrap().deadline, 1545);
                            source_schedule.advance(
                                source_cpu,
                                source_memory,
                                &mut cartridge,
                                &mut [],
                                SoundInterval::new(1544..1545, source_audio),
                            );
                            assert_eq!(source_schedule.fault, None);
                            let expected = if change == 1 {
                                if target < 0x300000 { 0x69 } else { 0xff }
                            } else if target & 1 == 0 {
                                0xff
                            } else if change == 2 {
                                0x42
                            } else {
                                0x37
                            };
                            assert_eq!(source_cpu.regs().a, expected);
                            assert_eq!(source_memory.ram[0x777], 0xa6);
                            assert_eq!(source_memory.bank, (target >> 15) as u16);
                            assert!(source_schedule.pending.is_none());
                        }
                        assert_eq!(cpu, restored_cpu);
                        assert_eq!(schedule, restored);
                        assert_eq!(memory, restored_memory);
                        assert_eq!(audio, restored_audio);
                        assert_eq!(cartridge.save.as_ref().unwrap().bytes, cells);
                    }
                }
            }
        }
    }
}

fn lock_on_cartridge(active: bool) -> Cartridge {
    use crate::cartridge::registers::LockOn;
    let bytes = crate::cartridge::registers::tests::normal_rom_fixture(LockOn::MiB);
    let mut cartridge = Cartridge::new(bytes).unwrap();
    cartridge.lock_on = Some(if active { LockOn::Patch } else { LockOn::MiB });
    cartridge
}

fn lock_on_publication_cases() {
    for active in [false, true] {
        for target in [0x300001, 0x33ffff, 0x340001, 0x3fffff] {
            for indirect in [false, true] {
                for requester in [Requester::MAIN, Requester::VDP] {
                    let mut cartridge = lock_on_cartridge(active);
                    let (mut cpu, mut memory, mut schedule, mut audio, cut) =
                        setup(target, indirect);
                    schedule.advance(
                        &mut cpu,
                        &mut memory,
                        &mut cartridge,
                        &mut [],
                        SoundInterval::new(0..cut, &mut audio),
                    );
                    assert_eq!(schedule.fault, None);
                    let pending = schedule.pending.as_ref().unwrap();
                    assert_eq!(pending.bank_access.unwrap().address, target);
                    assert!(pending.replay.is_some());
                    schedule.validate_cartridge(&cartridge).unwrap();
                    let mut writer = StateWriter::new();
                    schedule.write_state(&cpu, &mut writer).unwrap();
                    let mut restored = SoundSchedule::default();
                    let mut restored_cpu = Cpu::new();
                    restored
                        .read_state(
                            &mut restored_cpu,
                            &mut StateReader::new(&writer.into_bytes()),
                        )
                        .unwrap();
                    restored.validate_cartridge(&cartridge).unwrap();
                    assert_eq!(schedule, restored);
                    let mut unsafe_legacy = restored.clone();
                    unsafe_legacy.pending.as_mut().unwrap().replay = None;
                    let mut writer = StateWriter::new();
                    unsafe_legacy
                        .write_state(&restored_cpu, &mut writer)
                        .unwrap();
                    let mut decoded = SoundSchedule::default();
                    let mut decoded_cpu = restored_cpu.clone();
                    decoded
                        .read_state(
                            &mut decoded_cpu,
                            &mut StateReader::new(&writer.into_bytes()),
                        )
                        .unwrap();
                    let before = decoded.clone();
                    assert!(decoded.validate_cartridge(&cartridge).is_err());
                    assert_eq!(decoded, before);
                    let mut missing_reservation = restored.clone();
                    missing_reservation.pending.as_mut().unwrap().bank_access = None;
                    assert!(missing_reservation.validate_cartridge(&cartridge).is_err());
                    assert!(
                        cartridge
                            .lock_on
                            .as_mut()
                            .unwrap()
                            .write_control(u8::from(!active))
                    );
                    memory.bank = 0x1ff;
                    memory.ram[0x777] = 0xa6;
                    let mut other_memory = memory.clone();
                    let mut other_audio = audio.clone();
                    let occupied = [Reservation {
                        requester,
                        ticks: (cut - 1)..1500,
                    }];
                    for (cpu, memory, schedule, audio) in [
                        (&mut cpu, &mut memory, &mut schedule, &mut audio),
                        (
                            &mut restored_cpu,
                            &mut other_memory,
                            &mut restored,
                            &mut other_audio,
                        ),
                    ] {
                        schedule.advance(
                            cpu,
                            memory,
                            &mut cartridge,
                            &mut [],
                            SoundInterval {
                                ticks: cut..1544,
                                occupied: &occupied,
                                sound_irq: None,
                                audio,
                            },
                        );
                        assert_eq!(schedule.fault, None);
                        assert_eq!(cpu.regs().a, 0);
                        assert_eq!(schedule.pending.as_ref().unwrap().deadline, 1545);
                        schedule.advance(
                            cpu,
                            memory,
                            &mut cartridge,
                            &mut [],
                            SoundInterval::new(1544..1545, audio),
                        );
                        assert_eq!(schedule.fault, None);
                        let expected = if active {
                            0x28 + ((target & 0xfffff) / 0x40000) as u8
                        } else {
                            0x2c
                        };
                        assert_eq!(cpu.regs().a, expected);
                        assert_eq!(memory.bank, (target >> 15) as u16);
                        assert_eq!(memory.ram[0x777], 0xa6);
                        assert!(schedule.pending.is_none());
                    }
                    assert_eq!(
                        (cpu, memory, schedule, audio),
                        (restored_cpu, other_memory, restored, other_audio)
                    );
                }
            }
        }
    }
}

fn lock_on_fetch_cases() {
    use crate::cartridge::registers::LockOn;
    use zeff_emu_common::audio_trace::MdTraceSource;
    for sequence in [
        &[0][..],
        &[0xcb, 0],
        &[0xed, 0x44],
        &[0xdd, 0],
        &[0xfd, 0],
        &[0xdd, 0xfd, 0],
    ] {
        for phase in 0..sequence.len() {
            for active in [false, true] {
                for changed in [false, true] {
                    for requester in [Requester::MAIN, Requester::VDP] {
                        let mut bytes = lock_on_cartridge(active).bytes.to_vec();
                        bytes[0x200000..0x200000 + sequence.len()].copy_from_slice(sequence);
                        bytes[0x300000..0x300000 + sequence.len()].copy_from_slice(sequence);
                        if changed {
                            bytes[(if active { 0x200000 } else { 0x300000 }) + phase] ^= 1;
                        }
                        let mut cartridge = Cartridge::new(bytes).unwrap();
                        cartridge.lock_on = Some(if active { LockOn::Patch } else { LockOn::MiB });
                        let mut cpu =
                            Cpu::new_with_reset(zeff_z80::ResetState::new(0x8000, 0x1ffe));
                        let mut memory = SoundMemory {
                            bank: 0x60,
                            ..SoundMemory::default()
                        };
                        memory.set_reset(false);
                        let mut schedule = SoundSchedule::default();
                        let mut audio = Audio::default();
                        let start = phase as u64 * 60;
                        schedule.advance(
                            &mut cpu,
                            &mut memory,
                            &mut cartridge,
                            &mut [],
                            SoundInterval::new(0..start + 1, &mut audio),
                        );
                        assert_eq!(schedule.fault, None);
                        let pending = schedule.pending.as_ref().unwrap();
                        assert_eq!(
                            (pending.instruction.cycles, pending.replay.unwrap().accesses),
                            (4, 1)
                        );
                        assert_eq!(
                            pending.bank_access.unwrap().address,
                            0x300000 + phase as u32
                        );
                        let occupied = [Reservation {
                            requester,
                            ticks: start..1000,
                        }];
                        schedule.advance(
                            &mut cpu,
                            &mut memory,
                            &mut cartridge,
                            &mut [],
                            SoundInterval {
                                ticks: start + 1..1001,
                                occupied: &occupied,
                                sound_irq: None,
                                audio: &mut audio,
                            },
                        );
                        assert_eq!(schedule.fault, None);
                        let pending = schedule.pending.as_ref().unwrap();
                        let deadline = pending.deadline;
                        assert!(deadline > 1001);
                        let expected = pending.cpu.clone();
                        let mut origin_probe = pending.clone();
                        origin_probe.trace_origin =
                            Some(crate::hardware::audio::trace::sound_origin(
                                cpu.regs().pc,
                                memory.bank,
                                &cartridge,
                            ));
                        assert!(
                            cartridge
                                .lock_on
                                .as_mut()
                                .unwrap()
                                .write_control(u8::from(!active))
                        );
                        memory.bank = 0x1ff;
                        let mut writer = StateWriter::new();
                        schedule.write_state(&cpu, &mut writer).unwrap();
                        let mut restored = SoundSchedule::default();
                        let mut restored_cpu = Cpu::new();
                        restored
                            .read_state(
                                &mut restored_cpu,
                                &mut StateReader::new(&writer.into_bytes()),
                            )
                            .unwrap();
                        restored.validate_cartridge(&cartridge).unwrap();
                        if !changed {
                            origin_probe
                                .refresh_reads(&cpu, &memory, &cartridge, &audio)
                                .unwrap();
                            assert_eq!(
                                origin_probe.trace_origin.unwrap().source,
                                MdTraceSource::Unknown
                            );
                        }
                        let before = (
                            cpu.clone(),
                            memory.clone(),
                            cartridge.clone(),
                            schedule.state,
                        );
                        let mut other_memory = memory.clone();
                        let mut other_audio = audio.clone();
                        for (cpu, memory, schedule, audio) in [
                            (&mut cpu, &mut memory, &mut schedule, &mut audio),
                            (
                                &mut restored_cpu,
                                &mut other_memory,
                                &mut restored,
                                &mut other_audio,
                            ),
                        ] {
                            schedule.advance(
                                cpu,
                                memory,
                                &mut cartridge,
                                &mut [],
                                SoundInterval::new(1001..deadline, audio),
                            );
                            if changed {
                                assert!(schedule.fault.is_some());
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
                                assert_eq!(*cpu, expected);
                                assert_eq!(memory.bank, 0x60);
                            }
                            assert!(schedule.pending.is_none());
                        }
                        assert_eq!(
                            (cpu, memory, schedule, audio),
                            (restored_cpu, other_memory, restored, other_audio)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn cartridge_unbounded_opcode_and_inconsistent_replay_reads_are_rejected_transactionally() {
    let mut cartridge = cartridge(8192);
    let (cpu, memory, _, audio, _) = setup(0x200001, false);
    assert!(matches!(
        PendingSound::prepare(&cpu, &memory, &cartridge, 0),
        Err(SoundFault::UnsupportedAccess {
            address: 0x8001,
            direction: Direction::Read
        })
    ));
    let mut schedule = SoundSchedule::default();
    let mut cpu = zeff_z80::Cpu::new_with_reset(zeff_z80::ResetState::new(0x8001, 0x1ffe));
    let before = cpu.clone();
    let mut memory = memory;
    let before_memory = memory.clone();
    let mut audio = audio;
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(0..100, &mut audio),
    );
    assert!(matches!(
        schedule.fault,
        Some(SoundFault::UnsupportedAccess {
            address: 0x8001,
            direction: Direction::Read
        })
    ));
    assert_eq!(cpu, before);
    assert_eq!(memory, before_memory);
    let (mut cpu, mut memory, mut schedule, mut audio, cut) = setup(0x200001, false);
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(0..cut, &mut audio),
    );
    let mut pending = schedule.pending.take().unwrap();
    pending.replay.as_mut().unwrap().bank_identity = Some((0x200003, Direction::Read));
    let original = pending.clone();
    assert_eq!(
        pending.refresh_reads(&cpu, &memory, &cartridge, &audio),
        Err(SoundFault::UnstableCartridgeRead { pc: 0 })
    );
    assert_eq!(pending, original);
    for opcode in [0xdd, 0xfd] {
        let mut cartridge = cartridge.clone();
        std::sync::Arc::make_mut(&mut cartridge.bytes)[0x200001] = opcode;
        let cpu = zeff_z80::Cpu::new_with_reset(zeff_z80::ResetState::new(0x8001, 0x1ffe));
        assert!(matches!(
            PendingSound::prepare(&cpu, &memory, &cartridge, 0),
            Err(SoundFault::UnsupportedAccess {
                address: 0x8001,
                direction: Direction::Read
            })
        ));
    }
}

#[test]
fn scheduled_mapper_and_cell_writes_sample_in_bus_order_and_trace_once() {
    use crate::hardware::bus::{Bus, scheduled::ScheduledBus};
    use zeff_emu_common::audio_trace::{
        AudioTraceTiming, MdTraceOrigin, MdTraceSource, MdTraceWrite, MdTraceWriter,
    };
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
    use zeff_m68k::timing::ClockedBus;

    for size in [8192, 32768] {
        for indirect in [false, true] {
            for before_read in [false, true] {
                for write_cell in [false, true] {
                    let (mut cpu, memory, mut schedule, _, cut) = setup(0x200001, indirect);
                    let mut bus = Bus::new(cartridge(size), crate::Region::NtscUs);
                    bus.sound = memory;
                    let next_pc = if indirect { 4 } else { 3 };
                    bus.sound.ram[next_pc..next_pc + 4].copy_from_slice(&[0x32, 0x11, 0x7f, 0x76]);
                    let save = bus.cartridge.save.as_mut().unwrap();
                    save.enabled = write_cell;
                    save.bytes.fill(0x37);
                    bus.audio.trace = bus
                        .audio
                        .trace
                        .prepare_with_clock(
                            16,
                            bus.rate(),
                            crate::hardware::audio::trace::trace_chip(bus.region),
                            AudioTraceTiming::BusServiceBoundary,
                        )
                        .unwrap();
                    let mut scheduled = ScheduledBus::new(&mut bus, &mut cpu, &mut schedule);
                    scheduled
                        .advance_machine(if before_read { cut - 11 } else { cut })
                        .unwrap();
                    let access = BusAccess {
                        logical_address: if write_cell { 0x200001 } else { 0xa130f1 },
                        bus_address: if write_cell { 0x200000 } else { 0xa130f0 },
                        kind: AccessKind::Data,
                        lanes: ByteLanes::Lower,
                        function_code: FunctionCode::SupervisorData,
                    };
                    let clock = scheduled.bus.cpu_clock;
                    let result =
                        scheduled.write_cycle(access, if write_cell { 0x42 } else { 1 }, clock);
                    assert_eq!(result.result, Ok(()));
                    assert_eq!(scheduled.schedule.fault, None);
                    let expected = if before_read {
                        if write_cell { 0x42 } else { 0x37 }
                    } else if write_cell {
                        0x37
                    } else {
                        0x69
                    };
                    if before_read {
                        assert_eq!(scheduled.cpu.regs().a, 0);
                        let deadline = scheduled.schedule.pending.as_ref().unwrap().deadline;
                        let remaining = deadline - scheduled.bus.now.get();
                        scheduled.advance_machine(remaining - 1).unwrap();
                        assert_eq!(scheduled.cpu.regs().a, 0);
                        scheduled.advance_machine(1).unwrap();
                    } else {
                        assert!(result.wait_clocks > 0);
                    }
                    assert_eq!(scheduled.cpu.regs().a, expected);
                    scheduled.advance_machine(1000).unwrap();
                    assert_eq!(scheduled.schedule.fault, None);
                    let capture = scheduled
                        .bus
                        .audio
                        .trace
                        .finish(scheduled.bus.now.get())
                        .unwrap();
                    assert_eq!(capture.events.len(), 1);
                    assert_eq!(
                        capture.events[0].write,
                        MdTraceWrite::Psg {
                            value: expected,
                            origin: MdTraceOrigin {
                                writer: MdTraceWriter::Z80,
                                boundary_pc: (next_pc + 3) as u32,
                                source: MdTraceSource::SoundRam {
                                    offset: (next_pc + 3) as u16
                                }
                            },
                        }
                    );
                }
            }
        }
    }
}

#[test]
fn unused_cartridge_pending_read_keeps_reservation_across_waits_restore_and_bank_changes() {
    for target in [0x3fffff, 0x400000, 0x407fff, 0x7f8000, 0x7fffff] {
        for indirect in [false, true] {
            for requester in [Requester::MAIN, Requester::VDP] {
                let mut cartridge = cartridge(8192);
                cartridge.save = None;
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
                assert!(pending.any_bank_access);
                assert_eq!(
                    pending.bank_access.map(|access| access.address),
                    Some(target)
                );
                assert_eq!(
                    pending.reservation().map(|slot| slot.requester),
                    Some(Requester::SOUND)
                );
                assert!(pending.replay.is_none());
                assert_eq!(cpu.regs().a, 0);
                let mut writer = StateWriter::new();
                schedule.write_state(&cpu, &mut writer).unwrap();
                let mut restored = SoundSchedule::default();
                let mut restored_cpu = Cpu::new();
                restored
                    .read_state(
                        &mut restored_cpu,
                        &mut StateReader::new(&writer.into_bytes()),
                    )
                    .unwrap();
                assert_eq!(restored, schedule);
                memory.bank = 0x100;
                let mut restored_memory = memory.clone();
                let mut restored_audio = audio.clone();
                let occupied = [Reservation {
                    requester,
                    ticks: (cut - 1)..1500,
                }];
                for (cpu, memory, schedule, audio) in [
                    (&mut cpu, &mut memory, &mut schedule, &mut audio),
                    (
                        &mut restored_cpu,
                        &mut restored_memory,
                        &mut restored,
                        &mut restored_audio,
                    ),
                ] {
                    schedule.advance(
                        cpu,
                        memory,
                        &mut cartridge,
                        &mut [],
                        SoundInterval {
                            ticks: cut..1544,
                            occupied: &occupied,
                            sound_irq: None,
                            audio,
                        },
                    );
                    assert_eq!(schedule.fault, None);
                    assert_eq!(cpu.regs().a, 0);
                    assert_eq!(schedule.pending.as_ref().unwrap().deadline, 1545);
                    schedule.advance(
                        cpu,
                        memory,
                        &mut cartridge,
                        &mut [],
                        SoundInterval::new(1544..1545, audio),
                    );
                    assert_eq!(schedule.fault, None);
                    assert_eq!(cpu.regs().a, 0xff);
                    assert_eq!(memory.bank, (target >> 15) as u16);
                    assert!(schedule.pending.is_none());
                }
                assert_eq!(cpu, restored_cpu);
                assert_eq!(memory, restored_memory);
                assert_eq!(schedule, restored);
            }
        }
    }
}

#[test]
fn unused_cartridge_indexed_ffff_operand_reads_ff_after_vdp_wait_and_restored_phase() {
    let mut cartridge = cartridge(8192);
    let mut memory = SoundMemory {
        bank: 0xff,
        ..SoundMemory::default()
    };
    memory.set_reset(false);
    memory.ram[..8].copy_from_slice(&[0xfd, 0x21, 0xff, 0xff, 0xfd, 0x7e, 0, 0x76]);
    let mut cpu = Cpu::new();
    let mut schedule = SoundSchedule::default();
    let mut audio = Audio::default();
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(0..451, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(cpu.regs().iy, 0xffff);
    assert_eq!(cpu.regs().a, 0);
    assert!(schedule.pending.as_ref().unwrap().any_bank_access);
    assert!(schedule.pending.as_ref().unwrap().replay.is_none());
    let mut writer = StateWriter::new();
    schedule.write_state(&cpu, &mut writer).unwrap();
    let mut restored = SoundSchedule::default();
    let mut restored_cpu = Cpu::new();
    restored
        .read_state(
            &mut restored_cpu,
            &mut StateReader::new(&writer.into_bytes()),
        )
        .unwrap();
    let occupied = [Reservation {
        requester: Requester::VDP,
        ticks: 449..750,
    }];
    let mut restored_memory = memory.clone();
    let mut restored_audio = audio.clone();
    for (cpu, memory, schedule, audio) in [
        (&mut cpu, &mut memory, &mut schedule, &mut audio),
        (
            &mut restored_cpu,
            &mut restored_memory,
            &mut restored,
            &mut restored_audio,
        ),
    ] {
        schedule.advance(
            cpu,
            memory,
            &mut cartridge,
            &mut [],
            SoundInterval {
                ticks: 451..794,
                occupied: &occupied,
                sound_irq: None,
                audio,
            },
        );
        assert_eq!(cpu.regs().a, 0);
        assert_eq!(schedule.pending.as_ref().unwrap().deadline, 795);
        schedule.advance(
            cpu,
            memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(794..795, audio),
        );
        assert_eq!(schedule.fault, None);
        assert_eq!(cpu.regs().a, 0xff);
        assert_eq!(cpu.regs().pc, 7);
        assert_eq!(memory.bank, 0xff);
    }
    assert_eq!(cpu, restored_cpu);
    assert_eq!(schedule, restored);
}

#[test]
fn unused_cartridge_neighbor_rejections_preserve_candidate_cpu_memory_and_audio() {
    let cartridge = cartridge(8192);
    for target in [0x800000, 0xbfffff, 0xdfffff] {
        let (cpu, memory, _, audio, _) = setup(target, false);
        let before = (cpu.clone(), memory.clone(), audio.clone());
        assert!(matches!(
            PendingSound::prepare(&cpu, &memory, &cartridge, 0),
            Err(SoundFault::UnsupportedAccess {
                direction: Direction::Read,
                ..
            })
        ));
        assert_eq!((cpu, memory, audio), before);
    }
}
