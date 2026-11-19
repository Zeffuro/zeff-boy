use super::*;

fn canonical_bus(machine: &Emulator) -> Vec<u8> {
    let mut bus = machine.bus.clone();
    bus.audio.flush_pending();
    let mut writer = zeff_emu_common::save_state::StateWriter::new();
    bus.write_state(&mut writer).unwrap();
    writer.into_bytes()
}

#[cfg_attr(test, test)]
pub(crate) fn raw17_empty_partial_full_restore_and_clone_continue_exactly() {
    for concurrent in [false, true] {
        for reset in [false, true] {
            for history in [
                Prefetch::Empty,
                Prefetch::UpperOnly(0x12),
                Prefetch::Full(0x1234),
            ] {
                let mut source = machine(&[0x4e71; 8], concurrent, reset, Region::Pal);
                source.bus.prefetch = history;
                let bytes = source.encode_state().unwrap();
                assert_eq!(
                    &bytes[8..12],
                    &crate::save_state::STATE_VERSION.to_le_bytes()
                );
                let saved = bytes;
                let mut restored = source.clone();
                let expected_source = source.clone();
                restored.decode_state(&saved).unwrap();
                assert_eq!(restored.bus.prefetch, history);
                assert_eq!(
                    (&restored.encode_state().unwrap()),
                    (&expected_source.encode_state().unwrap())
                );
                for (lanes, expected) in [
                    (
                        ByteLanes::Both,
                        if history == Prefetch::Full(0x1234) {
                            Ok(0x1234)
                        } else {
                            failure(0xa00044)
                        },
                    ),
                    (
                        ByteLanes::Upper,
                        if history == Prefetch::Empty {
                            failure(0xa00044)
                        } else {
                            Ok(0x1200)
                        },
                    ),
                    (
                        ByteLanes::Lower,
                        if history == Prefetch::Full(0x1234) {
                            Ok(0x34)
                        } else {
                            failure(0xa00044)
                        },
                    ),
                ] {
                    let mut actual = expected_source.clone();
                    let mut target = restored.clone();
                    for _ in 0..2 {
                        assert_eq!(
                            read(&mut actual, access(0xa00044, AccessKind::Data, lanes)).result,
                            expected
                        );
                        assert_eq!(
                            read(&mut target, access(0xa00044, AccessKind::Data, lanes)).result,
                            expected
                        );
                        assert_eq!(canonical_bus(&actual), canonical_bus(&target));
                        assert_eq!(actual.cpu, target.cpu);
                        assert_eq!(actual.sound_cpu, target.sound_cpu);
                        assert_eq!(actual.sound_schedule, target.sound_schedule);
                        assert_eq!(actual.main_stopped, target.main_stopped);
                        assert_eq!(actual.concurrent_sound, target.concurrent_sound);
                        assert_eq!(
                            actual.bus.prefetch,
                            if expected.is_ok() {
                                history
                            } else {
                                Prefetch::Empty
                            }
                        );
                    }
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
pub(crate) fn owned_reads_and_other_observation_consumers_keep_their_contracts() {
    indexed_rom_read_reserves_before_prefetch_and_request();
    for concurrent in [false, true] {
        let mut owned = machine(&[0x4e71; 8], concurrent, false, Region::NtscUs);
        owned.bus.sound.request(true);
        owned.bus.sound.complete_boundary();
        assert!(owned.bus.sound.granted);
        owned.bus.sound.ram[0x44..0x46].copy_from_slice(&[0xa5, 0x6b]);
        for (lanes, expected) in [
            (ByteLanes::Both, 0xa5a5),
            (ByteLanes::Upper, 0xa5a5),
            (ByteLanes::Lower, 0x6b6b),
        ] {
            seed(&mut owned);
            assert_eq!(
                read(&mut owned, access(0xa00044, AccessKind::Data, lanes)).result,
                Ok(expected)
            );
            assert_eq!(owned.bus.prefetch, Prefetch::Empty);
        }
        seed(&mut owned);
        assert_eq!(
            read(
                &mut owned,
                access(0xa07f20, AccessKind::Data, ByteLanes::Both)
            )
            .result,
            failure(0xa07f20)
        );
        assert_eq!(owned.bus.prefetch, Prefetch::Empty);
        for operation in 0..6 {
            let mut machine = machine(&[0x4e71; 4], concurrent, true, Region::NtscUs);
            seed(&mut machine);
            match operation {
                0 => assert_eq!(
                    read(
                        &mut machine,
                        access(SOUND_RESET, AccessKind::Data, ByteLanes::Both)
                    )
                    .result,
                    Ok(0x1234)
                ),
                1 => assert_eq!(
                    read(
                        &mut machine,
                        access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Upper)
                    )
                    .result,
                    Ok(0x1300)
                ),
                2 => assert_eq!(
                    read(
                        &mut machine,
                        access(0xc00004, AccessKind::Data, ByteLanes::Both)
                    )
                    .result
                    .unwrap()
                        & VDP_STATUS_FLOAT_MASK,
                    0x1200 & VDP_STATUS_FLOAT_MASK
                ),
                3 => assert_eq!(
                    read(
                        &mut machine,
                        access(0xa00044, AccessKind::Opcode, ByteLanes::Both)
                    )
                    .result,
                    failure(0xa00044)
                ),
                4 => {
                    write(
                        &mut machine,
                        access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Both),
                        0x100,
                    )
                    .result
                    .unwrap();
                }
                _ => {
                    write(
                        &mut machine,
                        access(0xa06000, AccessKind::Data, ByteLanes::Both),
                        0x0100,
                    )
                    .result
                    .unwrap();
                    assert_eq!(machine.bus.sound.bank, 0x155);
                }
            }
            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
        }
    }
}

fn indexed_rom_read_reserves_before_prefetch_and_request() {
    for prefix in [0xdd, 0xfd] {
        let mut source = machine(&[0x1334; 8], true, false, Region::NtscJapan);
        source.bus.sound.bank = 0;
        source.bus.sound.ram[..8]
            .copy_from_slice(&[prefix, 0x21, 0xe5, 0x80, prefix, 0x66, 0x1b, 0x76]);
        for _ in 0..600 {
            ScheduledBus::new(
                &mut source.bus,
                &mut source.sound_cpu,
                &mut source.sound_schedule,
            )
            .advance_machine(1)
            .unwrap();
            assert_eq!(source.sound_fault(), None);
            if source
                .sound_schedule
                .pending
                .as_ref()
                .is_some_and(|pending| {
                    pending.instruction.opcode == prefix && pending.instruction.cycles == 5
                })
            {
                break;
            }
        }
        let address_phase = source.sound_schedule.pending.as_ref().unwrap();
        assert_eq!(address_phase.instruction.cycles, 5);
        assert!(!address_phase.any_bank_access);
        let before_store = address_phase.deadline - 14;
        let ticks = before_store - source.bus.now.get();
        ScheduledBus::new(
            &mut source.bus,
            &mut source.sound_cpu,
            &mut source.sound_schedule,
        )
        .advance_machine(ticks)
        .unwrap();
        let store = write(
            &mut source,
            access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Both),
            0x100,
        );
        assert_eq!(store.result, Ok(()));
        assert_eq!(store.wait_clocks, 0);
        assert!(source.bus.sound.requested && !source.bus.sound.granted);
        let pending = source.sound_schedule.pending.as_ref().unwrap();
        let bank = pending.bank_access.unwrap();
        assert_eq!(
            (bank.address, bank.direction, bank.value),
            (0x100, Direction::Read, 0x13)
        );
        assert_eq!(
            pending.reservation().unwrap().ticks,
            pending.start..pending.deadline
        );
        source.cpu = PrefetchCpu::from_prefetched(
            source.registers(),
            source.cpu.queued_words().unwrap(),
            source.bus.cpu_clock,
        )
        .unwrap();
        let raw = source.encode_state().unwrap();
        assert_eq!(&raw[8..12], &crate::save_state::STATE_VERSION.to_le_bytes());
        let mut restored = source.clone();
        restored.decode_state(&raw).unwrap();
        assert_eq!(restored.encode_state().unwrap(), raw);
        let mut legacy = source.clone();
        legacy.sound_schedule.pending.as_mut().unwrap().bank_access = None;
        let old = legacy.encode_state().unwrap();
        let mut old_restored = legacy.clone();
        old_restored.decode_state(&old).unwrap();
        assert_eq!((&old_restored.encode_state().unwrap()), (&old));
        assert!(
            old_restored
                .sound_schedule
                .pending
                .as_ref()
                .unwrap()
                .reservation()
                .is_none()
        );
        for machine in [&mut legacy, &mut old_restored] {
            let fetch = read(
                machine,
                access(0x100, AccessKind::Prefetch, ByteLanes::Both),
            );
            assert_eq!(fetch.result, Ok(0x1334));
            assert_eq!(fetch.wait_clocks, 0);
            assert!(machine.sound_schedule.pending.is_some());
            let status = read(
                machine,
                access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Both),
            );
            assert_eq!(status.result, failure(SOUND_BUS_REQUEST));
            assert_eq!(status.wait_clocks, 0);
            assert!(machine.bus.sound.granted);
            assert!(machine.sound_schedule.pending.is_none());
            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
        }
        assert_eq!(canonical_bus(&legacy), canonical_bus(&old_restored));
        assert_eq!(legacy.sound_cpu, old_restored.sound_cpu);
        assert_eq!(legacy.sound_schedule, old_restored.sound_schedule);
        assert_eq!(legacy.cpu, old_restored.cpu);
        assert_eq!(
            legacy.drain_audio_samples(),
            old_restored.drain_audio_samples()
        );
        for machine in [&mut source, &mut restored] {
            let fetch = read(
                machine,
                access(0x100, AccessKind::Prefetch, ByteLanes::Both),
            );
            assert_eq!(fetch.result, Ok(0x1334));
            assert!(fetch.wait_clocks > 0);
            assert_eq!(machine.sound_cpu.regs().h, 0x13);
            assert!(machine.bus.sound.granted);
            assert!(machine.sound_schedule.pending.is_none());
            assert_eq!(machine.bus.prefetch, Prefetch::Full(0x1334));
            let status = read(
                machine,
                access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Both),
            );
            assert_eq!(status.result, Ok(0x1234));
            assert_eq!(status.wait_clocks, 0);
            assert_eq!(machine.sound_fault(), None);
        }
        assert_eq!(canonical_bus(&source), canonical_bus(&restored));
        assert_eq!(source.sound_cpu, restored.sound_cpu);
        assert_eq!(source.sound_schedule, restored.sound_schedule);
        assert_eq!(source.cpu, restored.cpu);
        assert_eq!(source.drain_audio_samples(), restored.drain_audio_samples());
        assert_eq!(
            read(
                &mut source,
                access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Lower)
            )
            .result,
            failure(SOUND_BUS_REQUEST)
        );
    }
}
