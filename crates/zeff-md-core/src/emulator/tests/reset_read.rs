use super::*;
use crate::constants::{SOUND_BUS_REQUEST, SOUND_RESET, VDP_STATUS_FLOAT_MASK};
use crate::hardware::sound::schedule::PendingSound;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CycleResult};

fn access(address: u32, kind: AccessKind, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address + u32::from(lanes == ByteLanes::Lower),
        bus_address: address,
        kind,
        lanes,
        function_code: FunctionCode::SupervisorData,
    }
}

fn read(machine: &mut Emulator, concurrent: bool, access: BusAccess) -> CycleResult<u16, BusError> {
    let clock = machine.bus.cpu_clock;
    if concurrent {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .read_cycle(access, clock)
    } else {
        machine.bus.read_cycle(access, clock)
    }
}

fn discarded(
    machine: &mut Emulator,
    concurrent: bool,
    access: BusAccess,
) -> CycleResult<(), BusError> {
    let clock = machine.bus.cpu_clock;
    if concurrent {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .read_discarded_cycle(access, clock)
    } else {
        machine.bus.read_discarded_cycle(access, clock)
    }
}

fn configured(program: &[u16], region: Region, concurrent_sound: bool) -> Emulator {
    let mut image = vec![0; 0x200];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in program.iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(
        image,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap()
}

fn discarded_reads_and_clear_continuations() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for reset in [false, true] {
                for request in [false, true] {
                    for history in [
                        Prefetch::Empty,
                        Prefetch::UpperOnly(0xa7),
                        Prefetch::Full(0x00a5),
                        Prefetch::Full(0x01a5),
                        Prefetch::Full(0xfe5a),
                    ] {
                        for code in [FunctionCode::UserData, FunctionCode::SupervisorData] {
                            let mut machine = configured(&[0x4e71; 2], region, concurrent);
                            machine.bus.sound.set_reset(reset);
                            machine.bus.sound.request(request);
                            machine.bus.sound.complete_boundary();
                            machine.bus.prefetch = history;
                            let sound = machine.bus.sound.clone();
                            let before = machine.bus.now.get();
                            let mut transfer =
                                access(SOUND_RESET, AccessKind::Data, ByteLanes::Upper);
                            transfer.function_code = code;
                            let mut ordinary = machine.clone();
                            assert_eq!(
                                read(&mut ordinary, concurrent, transfer).result,
                                history.sample(ByteLanes::Upper).ok_or(
                                    BusError::UnsupportedAccess {
                                        address: SOUND_RESET,
                                        direction: Direction::Read,
                                    }
                                )
                            );
                            assert_eq!(ordinary.bus.prefetch, Prefetch::Empty);
                            assert_eq!(ordinary.bus.sound, sound);
                            let lower_access =
                                access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Lower);
                            let lower_expected = history.sample(ByteLanes::Lower).ok_or(
                                BusError::UnsupportedAccess {
                                    address: SOUND_BUS_REQUEST,
                                    direction: Direction::Read,
                                },
                            );
                            let mut lower_discard = machine.clone();
                            assert_eq!(
                                discarded(&mut lower_discard, concurrent, lower_access).result,
                                lower_expected.clone().map(|_| ())
                            );
                            assert_eq!(lower_discard.bus.prefetch, Prefetch::Empty);
                            let mut lower = machine.clone();
                            assert_eq!(
                                read(
                                    &mut lower,
                                    concurrent,
                                    access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Lower)
                                )
                                .result,
                                lower_expected
                            );
                            let result = discarded(&mut machine, concurrent, transfer);
                            assert_eq!(result.result, Ok(()));
                            assert_eq!(result.wait_clocks, 0);
                            assert_eq!(machine.bus.now.get() - before, 28);
                            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                            assert_eq!(machine.bus.sound, sound);
                            assert_eq!(machine.peek(SOUND_RESET), None);
                            assert_eq!(
                                discarded(
                                    &mut machine,
                                    concurrent,
                                    access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Lower)
                                )
                                .result,
                                failure(SOUND_BUS_REQUEST).map(|_| ())
                            );
                        }
                    }
                }
            }
            let mut negatives = vec![access(SOUND_RESET, AccessKind::Data, ByteLanes::Lower)];
            for address in [
                SOUND_RESET + 2,
                SOUND_RESET + 0xfe,
                0xa11000,
                0xa11300,
                0xb11200,
            ] {
                negatives.push(access(address, AccessKind::Data, ByteLanes::Upper));
            }
            for kind in [
                AccessKind::Opcode,
                AccessKind::Extension,
                AccessKind::ResetVector,
                AccessKind::Vector,
                AccessKind::Stack,
                AccessKind::Discard,
                AccessKind::Prefetch,
                AccessKind::InterruptAcknowledge,
            ] {
                negatives.push(access(SOUND_RESET, kind, ByteLanes::Upper));
            }
            for function_code in [
                FunctionCode::UserProgram,
                FunctionCode::SupervisorProgram,
                FunctionCode::CpuSpace,
            ] {
                let mut transfer = access(SOUND_RESET, AccessKind::Data, ByteLanes::Upper);
                transfer.function_code = function_code;
                negatives.push(transfer);
            }
            for transfer in negatives {
                let mut machine = configured(&[0x4e71; 2], region, concurrent);
                machine.bus.prefetch = Prefetch::Full(0xa55a);
                let before = machine.bus.now.get();
                assert_eq!(
                    discarded(&mut machine, concurrent, transfer).result,
                    failure(transfer.bus_address).map(|_| ())
                );
                assert_eq!(machine.bus.now.get() - before, 28);
                assert_eq!(machine.bus.prefetch, Prefetch::Empty);
            }
            let mut failed = configured(&[0x4e71; 2], region, concurrent);
            failed.bus.prefetch = Prefetch::Full(0xa55a);
            failed.bus.clock_fault = true;
            let saved = failed.clone();
            assert_eq!(
                discarded(
                    &mut failed,
                    concurrent,
                    access(SOUND_RESET, AccessKind::Data, ByteLanes::Upper)
                )
                .result,
                Err(BusError::ClockOverflow)
            );
            assert!(failed == saved);
            let mut owned = configured(&[0x4e71; 2], region, concurrent);
            let lock = access(0xff0200, AccessKind::Data, ByteLanes::Upper);
            let clock = owned.bus.cpu_clock;
            let acquired = if concurrent {
                ScheduledBus::new(
                    &mut owned.bus,
                    &mut owned.sound_cpu,
                    &mut owned.sound_schedule,
                )
                .begin_rmw(lock, clock)
            } else {
                owned.bus.begin_rmw(lock, clock)
            };
            assert_eq!(acquired, Ok(zeff_m68k::bus::BusResponse::Complete(())));
            let saved = owned.clone();
            assert_eq!(
                read(
                    &mut owned,
                    concurrent,
                    access(SOUND_RESET, AccessKind::Data, ByteLanes::Upper)
                )
                .result,
                failure(SOUND_RESET)
            );
            assert_eq!(owned, saved);
            assert_eq!(
                discarded(
                    &mut owned,
                    concurrent,
                    access(SOUND_RESET, AccessKind::Data, ByteLanes::Upper)
                )
                .result,
                failure(SOUND_RESET).map(|_| ())
            );
            assert!(owned == saved);
            let mut source = configured(
                &[0x4239, 0x00a1, 0x1200, 0x7245, 0x60fe, 0x4e71],
                region,
                concurrent,
            );
            source.set_sample_rate(48_000);
            source.bus.sound.set_reset(false);
            source.bus.sound.request(true);
            source.bus.sound.complete_boundary();
            source.bus.sound.ram[0x123] = 0xa5;
            source.cpu.registers.set_status_register(0x201f);
            let mut cloned = source.clone();
            let before = source.bus.now.get();
            let clock = source.cpu_clock();
            source.step_instruction().unwrap();
            cloned.step_instruction().unwrap();
            assert!(source == cloned);
            assert_eq!(source.cpu_clock().0 - clock.0, 20);
            assert_eq!(source.bus.now.get() - before, 140);
            assert_eq!(source.registers().pc, 0x106);
            assert_eq!(source.registers().status_register() & 0x1f, 0x14);
            assert_eq!(source.cpu.queued_words(), Some([0x7245, 0x60fe]));
            assert!(source.sound_reset_asserted());
            assert!(source.sound_bus_requested());
            assert!(!source.sound_bus_granted());
            assert_eq!(source.bus.sound.ram[0x123], 0xa5);
            assert_eq!(source.bus.prefetch, Prefetch::Empty);
            for frontend in [false, true] {
                let mut expected = configured(
                    &[0x4239, 0x00a1, 0x1200, 0x7245, 0x60fe, 0x4e71],
                    region,
                    concurrent,
                );
                expected.set_sample_rate(48_000);
                for value in [0x85, 0x02, 0x90] {
                    expected.bus.audio.write_psg(expected.bus.now.get(), value);
                }
                expected.bus.sound.set_reset(false);
                expected.bus.sound.request(true);
                expected.bus.sound.complete_boundary();
                let state = if frontend {
                    expected.encode_frontend_state().unwrap()
                } else {
                    expected.encode_state().unwrap()
                };
                let mut restored = expected.clone();
                restored.step_instruction().unwrap();
                if frontend {
                    restored.decode_frontend_state(&state).unwrap();
                } else {
                    restored.decode_state(&state).unwrap();
                    expected.decode_state(&state).unwrap();
                }
                expected.step_instruction().unwrap();
                restored.step_instruction().unwrap();
                assert_eq!(restored.registers().pc, 0x106);
                assert!(restored.sound_reset_asserted());
                for _ in 0..2 {
                    expected.step_field().unwrap();
                    restored.step_field().unwrap();
                    assert_eq!(
                        restored.encode_state().unwrap(),
                        expected.encode_state().unwrap()
                    );
                    assert_eq!(restored.framebuffer(), expected.framebuffer());
                    let pcm = expected.drain_audio_samples();
                    assert!(!pcm.is_empty());
                    assert!(pcm.iter().any(|sample| sample.abs() > 0.000001));
                    assert_eq!(restored.drain_audio_samples(), pcm);
                }
            }
        }
    }
}

fn seed(machine: &mut Emulator, concurrent: bool, word: u16) {
    machine.bus.ram[0x100..0x102].copy_from_slice(&word.to_be_bytes());
    assert_eq!(
        read(
            machine,
            concurrent,
            access(0xff0100, AccessKind::Prefetch, ByteLanes::Both)
        )
        .result,
        Ok(word)
    );
    assert_eq!(machine.bus.prefetch, Prefetch::Full(word));
    machine.bus.ram[0x100..0x102].copy_from_slice(&(!word).to_be_bytes());
}

fn failure(address: u32) -> Result<u16, BusError> {
    Err(BusError::UnsupportedAccess {
        address,
        direction: Direction::Read,
    })
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_read_returns_observed_full_word_once_without_reset_side_effects() {
    discarded_reads_and_clear_continuations();
    upper_reset_release();
    for concurrent in [false, true] {
        for reset in [false, true] {
            for word in [0x00a5, 0x01a5, 0xfe5a, 0xff5a, 0x7a55, 0x4e71] {
                for code in [FunctionCode::UserData, FunctionCode::SupervisorData] {
                    let mut machine = super::machine(&[0x4e71; 2]);
                    machine.bus.sound.request(true);
                    machine.bus.sound.set_reset(reset);
                    machine.bus.sound.complete_boundary();
                    seed(&mut machine, concurrent, word);
                    let sound = machine.bus.sound.clone();
                    let before = machine.bus.now;
                    let mut transfer = access(SOUND_RESET, AccessKind::Data, ByteLanes::Both);
                    transfer.function_code = code;
                    let result = read(&mut machine, concurrent, transfer);
                    assert_eq!(result.result, Ok(word));
                    assert_eq!(result.wait_clocks, 0);
                    assert_eq!(machine.bus.now.get() - before.get(), 28);
                    assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                    assert_eq!(machine.bus.sound, sound);
                    assert_eq!(machine.peek(SOUND_RESET), None);
                    assert_eq!(
                        read(&mut machine, concurrent, transfer).result,
                        failure(SOUND_RESET)
                    );
                }
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_read_requires_canonical_data_word_and_full_history() {
    for concurrent in [false, true] {
        let mut machine = super::machine(&[0x4e71; 2]);
        for offset in (0..=0xfe).step_by(2) {
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                if offset == 0 && lanes != ByteLanes::Lower {
                    continue;
                }
                seed(&mut machine, concurrent, 0x01a5);
                let before = machine.bus.now;
                assert_eq!(
                    read(
                        &mut machine,
                        concurrent,
                        access(SOUND_RESET + offset, AccessKind::Data, lanes)
                    )
                    .result,
                    failure(SOUND_RESET + offset)
                );
                assert_eq!(machine.bus.now.get() - before.get(), 28);
                assert_eq!(machine.bus.prefetch, Prefetch::Empty);
            }
        }
        for kind in [
            AccessKind::Opcode,
            AccessKind::Extension,
            AccessKind::ResetVector,
            AccessKind::Vector,
            AccessKind::Stack,
            AccessKind::Discard,
            AccessKind::Prefetch,
            AccessKind::InterruptAcknowledge,
        ] {
            seed(&mut machine, concurrent, 0x01a5);
            assert_eq!(
                read(
                    &mut machine,
                    concurrent,
                    access(SOUND_RESET, kind, ByteLanes::Both)
                )
                .result,
                failure(SOUND_RESET)
            );
            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
        }
        for address in [0xa11000, 0xa11300, 0xb11200] {
            seed(&mut machine, concurrent, 0x01a5);
            assert_eq!(
                read(
                    &mut machine,
                    concurrent,
                    access(address, AccessKind::Data, ByteLanes::Both)
                )
                .result,
                failure(address)
            );
        }
        for logical_address in [SOUND_RESET + 1, SOUND_RESET | 0x1000000] {
            seed(&mut machine, concurrent, 0x4e71);
            let transfer = BusAccess {
                logical_address,
                ..access(SOUND_RESET, AccessKind::Data, ByteLanes::Upper)
            };
            assert_eq!(
                read(&mut machine, concurrent, transfer).result,
                failure(SOUND_RESET)
            );
            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
        }
        for history in [
            Prefetch::Empty,
            Prefetch::UpperOnly(0x00),
            Prefetch::UpperOnly(0xff),
        ] {
            machine.bus.prefetch = history;
            assert_eq!(
                read(
                    &mut machine,
                    concurrent,
                    access(SOUND_RESET, AccessKind::Data, ByteLanes::Both)
                )
                .result,
                failure(SOUND_RESET)
            );
            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_read_keeps_local_history_even_when_grant_finishes() {
    for lanes in [ByteLanes::Upper, ByteLanes::Both] {
        for request in [false, true] {
            let mut machine = super::machine(&[0x4e71; 2]);
            machine.bus.sound.set_reset(false);
            let pending = PendingSound::prepare(
                &machine.sound_cpu,
                &machine.bus.sound,
                &machine.bus.cartridge,
                machine.bus.now.get(),
            )
            .unwrap();
            assert_eq!(pending.deadline, 105);
            machine.sound_schedule.pending = Some(pending);
            machine.bus.sound.request(request);
            seed(&mut machine, true, 0x01a5);
            assert!(!machine.sound_bus_granted());
            assert_eq!(
                read(
                    &mut machine,
                    true,
                    access(SOUND_RESET, AccessKind::Data, lanes)
                )
                .result,
                Ok(if lanes == ByteLanes::Upper {
                    0x0100
                } else {
                    0x01a5
                })
            );
            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
            assert_eq!(machine.sound_bus_granted(), request);
            assert!(!machine.sound_reset_asserted());
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn external_dma_wait_consumes_history_for_reset_and_existing_floating_readers() {
    for concurrent in [false, true] {
        for (address, lanes) in [
            (SOUND_RESET, ByteLanes::Both),
            (SOUND_RESET, ByteLanes::Upper),
            (SOUND_BUS_REQUEST, ByteLanes::Upper),
            (0xc00004, ByteLanes::Both),
        ] {
            let mut machine = super::machine(&[0x4e71; 2]);
            super::vdp_dma::configure(&mut machine, 17, 0, 2);
            machine.bus.advance_machine(1).unwrap();
            let write = machine
                .bus
                .vdp
                .queue_register(1, 0x14, Region::NtscUs)
                .unwrap();
            machine
                .bus
                .advance_machine(write.publish_at.get() - machine.bus.now.get() + 1)
                .unwrap();
            seed(&mut machine, concurrent, 0xffa5);
            machine
                .bus
                .vdp
                .write_control(0x4000, Region::NtscUs)
                .unwrap();
            machine.bus.vdp.write_control(0x80, Region::NtscUs).unwrap();
            let mut completion = machine.clone();
            let completed = discarded(
                &mut completion,
                concurrent,
                access(SOUND_RESET, AccessKind::Data, ByteLanes::Upper),
            );
            assert_eq!(completed.result, Ok(()));
            assert!(completed.wait_clocks > 0);
            assert_eq!(completion.bus.prefetch, Prefetch::Empty);
            let result = read(
                &mut machine,
                concurrent,
                access(address, AccessKind::Data, lanes),
            );
            assert!(result.wait_clocks > 0);
            assert_eq!(completed.wait_clocks, result.wait_clocks);
            assert_eq!(completion.bus.now, machine.bus.now);
            match address {
                SOUND_RESET => assert_eq!(result.result, failure(SOUND_RESET)),
                SOUND_BUS_REQUEST => assert_eq!(result.result, Ok(0x100)),
                _ => assert_eq!(result.result.unwrap() & VDP_STATUS_FLOAT_MASK, 0),
            }
            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn failed_second_prime_restores_the_complete_prefetch_word() {
    for concurrent in [false, true] {
        let mut machine = super::machine(&[0x4e71; 2]);
        machine.concurrent_sound = concurrent;
        let mut rom = machine.bus.cartridge.bytes.to_vec();
        rom.resize(0x400000, 0);
        rom[0x3ffffe..].copy_from_slice(&0xa55a_u16.to_be_bytes());
        machine.bus.cartridge.bytes = rom.into();
        machine.bus.cartridge.initial.pc = 0x3ffffe;
        seed(&mut machine, concurrent, 0xfe5a);
        let saved = machine.clone();
        assert!(matches!(
            machine.reset_cpu(),
            Err(TimingFault::Bus {
                access: BusAccess {
                    bus_address: 0x400000,
                    kind: AccessKind::Prefetch,
                    ..
                },
                direction: Direction::Read,
                source: BusError::UnsupportedAccess {
                    address: 0x400000,
                    direction: Direction::Read
                },
            })
        ));
        assert_eq!(machine.bus.prefetch, Prefetch::Full(0xfe5a));
        assert_eq!(machine, saved);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reservation_wait_clears_full_history_independently_of_active_sound() {
    let mut machine = super::bank_reservation::ready(&[0x36, 0x5a, 0x76]);
    let clock = machine.bus.cpu_clock;
    ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    )
    .idle(clock, 4);
    let pending = machine.sound_schedule.pending.as_ref().unwrap();
    assert!(pending.reservation().is_some());
    let end = pending.deadline;
    machine.bus.sound.request(true);
    machine.bus.sound.complete_boundary();
    assert!(machine.sound_bus_granted());
    seed(&mut machine, false, 0xfe5a);
    assert_eq!(machine.bus.prefetch, Prefetch::Full(0xfe5a));
    let mut completion = machine.clone();
    let completed = discarded(
        &mut completion,
        true,
        access(SOUND_RESET, AccessKind::Data, ByteLanes::Upper),
    );
    assert_eq!(completed.result, Ok(()));
    assert!(completed.wait_clocks > 0);
    assert!(completion.bus.now.get() >= end + 28);
    assert_eq!(completion.bus.prefetch, Prefetch::Empty);
    let mut upper = machine.clone();
    let result = read(
        &mut machine,
        true,
        access(SOUND_RESET, AccessKind::Data, ByteLanes::Both),
    );
    assert!(result.wait_clocks > 0);
    assert_eq!(completed.wait_clocks, result.wait_clocks);
    assert_eq!(completion.bus.now, machine.bus.now);
    assert!(machine.bus.now.get() >= end + 28);
    assert_eq!(result.result, failure(SOUND_RESET));
    assert_eq!(machine.bus.prefetch, Prefetch::Empty);
    let upper_result = read(
        &mut upper,
        true,
        access(SOUND_RESET, AccessKind::Data, ByteLanes::Upper),
    );
    assert_eq!(upper_result.result, failure(SOUND_RESET));
    assert_eq!(upper_result.wait_clocks, result.wait_clocks);
    assert_eq!(upper.bus.now, machine.bus.now);
    assert_eq!(upper.bus.prefetch, Prefetch::Empty);
}

fn upper_reset_release() {
    let words = [
        0x08f9, 0, 0x00a1, 0x1200, 0x4e71, 0x4e71, 0x4e71, 0x4e71, 0x60fe, 0x4e71,
    ];
    for concurrent in [false, true] {
        let mut source = configured(&words, Region::NtscUs, concurrent);
        source.bus.sound.request(true);
        source.bus.sound.ram[0x123] = 0xa5;
        assert!(source.sound_reset_asserted());
        assert!(!source.sound_bus_granted());
        let mut cloned = source.clone();
        source.step_instruction().unwrap();
        cloned.step_instruction().unwrap();
        assert_eq!(source, cloned);
        assert_eq!(source.registers().pc, 0x108);
        assert_eq!(source.registers().status_register() & 4, 4);
        assert_eq!(source.cpu.queued_words(), Some([0x4e71; 2]));
        assert!(!source.sound_reset_asserted());
        assert!(source.sound_bus_requested());
        assert!(source.sound_bus_granted());
        assert_eq!(source.bus.sound.ram[0x123], 0xa5);
        let raw = source.encode_state().unwrap();
        let front = source.encode_frontend_state().unwrap();
        let mut restored = configured(&words, Region::NtscUs, concurrent);
        restored.decode_state(&raw).unwrap();
        let mut frontend = configured(&words, Region::NtscUs, concurrent);
        frontend.decode_frontend_state(&front).unwrap();
        for current in [&mut source, &mut cloned, &mut restored, &mut frontend] {
            for _ in 0..4 {
                current.step_instruction().unwrap();
            }
            assert!(!current.sound_reset_asserted());
        }
        let endpoint = source.encode_state().unwrap();
        for current in [&cloned, &restored, &frontend] {
            assert_eq!(current.encode_state().unwrap(), endpoint);
        }
    }
}
