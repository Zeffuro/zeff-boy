use super::*;
use zeff_m68k::bus::BusResponse;
use zeff_m68k::timing::InterruptVector;

fn fresh(region: Region, concurrent_sound: bool, looping: bool) -> Emulator {
    let mut rom = vec![0; 0x200];
    rom[..8].copy_from_slice(&[0, 0xff, 0x80, 0, 0, 0, 1, 0]);
    let words: &[u16] = if looping {
        &[
            0x33fc, 0x8120, 0x00c0, 0x0004, 0x3039, 0x00c0, 0x0004, 0x4a39, 0x00ff, 0x0000, 0x67f2,
            0x33fc, 0x8104, 0x00c0, 0x0004, 0x4e71, 0x60fc,
        ]
    } else {
        &[0x4e71, 0x4e71, 0x60fa]
    };
    for (index, word) in words.iter().enumerate() {
        rom[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap();
    machine.set_sample_rate(48_000);
    machine
}

fn idle(machine: &mut Emulator, clocks: u32) {
    let start = machine.bus.cpu_clock;
    if machine.concurrent_sound {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .idle(start, clocks);
    } else {
        machine.bus.idle(start, clocks);
    }
    assert!(!machine.bus.clock_fault);
}

fn control_cycle(machine: &mut Emulator, value: u16) -> CycleResult<(), BusError> {
    let start = machine.bus.cpu_clock;
    if machine.concurrent_sound {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .write_cycle(access(STATUS, ByteLanes::Both), value, start)
    } else {
        machine
            .bus
            .write_cycle(access(STATUS, ByteLanes::Both), value, start)
    }
}

fn control(machine: &mut Emulator, value: u16) {
    let response = control_cycle(machine, value);
    assert_eq!(response.result, Ok(()));
    assert_eq!(response.wait_clocks, 0);
}

fn acknowledge(machine: &mut Emulator) {
    let mut access = access(0xfffff6, ByteLanes::Both);
    access.kind = AccessKind::InterruptAcknowledge;
    access.function_code = FunctionCode::CpuSpace;
    let start = machine.bus.cpu_clock;
    let response = if machine.concurrent_sound {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .acknowledge_cycle(access, start)
    } else {
        machine.bus.acknowledge_cycle(access, start)
    };
    assert_eq!(
        response.result,
        Ok(BusResponse::Complete(InterruptVector::Autovector))
    );
    assert_eq!(response.wait_clocks, 0);
}

fn pending_at_samples(begin: u64, publication: u64, reads: [u64; 2]) -> [bool; 2] {
    let mut a = false;
    let mut b = false;
    let mut prepared_a = false;
    let mut prepared_b = false;
    let mut t6 = false;
    let mut clear = false;
    let mut pending = true;
    let mut mode4 = false;
    let mut result = [false; 2];
    for at in begin - begin % 10..=reads[1] + 28 {
        for (index, read) in reads.into_iter().enumerate() {
            if at == read + 25 {
                result[index] = pending;
            }
        }
        if at % 10 == 0 {
            a = prepared_a;
            b = prepared_b;
        }
        let asserted = reads
            .into_iter()
            .any(|read| (read + 7..read + 25).contains(&at));
        if asserted {
            t6 = true;
        } else if b {
            t6 = false;
        }
        if at % 10 == 0 {
            clear = b && !a;
            prepared_a = t6;
            prepared_b = a;
        }
        if at == publication {
            mode4 = true;
        }
        if clear && mode4 {
            pending = false;
        }
    }
    result
}

fn phased_reads() {
    for region in [Region::NtscUs, Region::Pal] {
        for concurrent in [false, true] {
            for phase in 0..20_u64 {
                for with_ack in [false, true] {
                    let mut machine = fresh(region, concurrent, false);
                    let begin = 224 * 3420 + 140 + phase * 7;
                    let clocks = u32::try_from((begin - machine.bus.now.get()) / 7).unwrap();
                    idle(&mut machine, clocks);
                    assert_eq!(machine.bus.now.get(), begin);
                    assert!(machine.vdp().interrupts().vertical_pending());
                    assert!(machine.vdp().interrupts().horizontal_pending());
                    control(&mut machine, 0x8120);
                    let end = begin + 28;
                    let publication = machine
                        .vdp()
                        .registers()
                        .pending()
                        .unwrap()
                        .publish_at
                        .get();
                    assert_eq!(publication, end + 20 - end % 20);
                    if with_ack {
                        acknowledge(&mut machine);
                        assert!(machine.vdp().interrupts().vertical_pending());
                    }
                    let read_begin = machine.bus.now.get();
                    let expected =
                        pending_at_samples(begin, publication, [read_begin, read_begin + 28]);
                    for (index, pending) in expected.into_iter().enumerate() {
                        let lanes = [ByteLanes::Both, ByteLanes::Upper, ByteLanes::Lower]
                            [phase as usize % 3];
                        let address = (if phase & 1 == 0 { STATUS } else { 0xd8ff06 })
                            + u32::from(lanes == ByteLanes::Lower);
                        machine.bus.prefetch = Prefetch::UpperOnly(0xa5 ^ index as u8);
                        let upper = if lanes == ByteLanes::Lower {
                            0
                        } else {
                            (u16::from(0xa5 ^ index as u8) << 8) & FLOAT_MASK
                        };
                        let result = transfer(&mut machine, access(address, lanes));
                        assert_eq!(
                            result.result,
                            Ok(upper
                                | 0x0208
                                | u16::from(region == Region::Pal)
                                | if pending { 0x80 } else { 0 })
                        );
                        assert_eq!(result.wait_clocks, 0);
                        assert_eq!(machine.bus.now.get(), read_begin + 28 * (index as u64 + 1));
                    }
                    assert!(machine.vdp().requires_timed_ports());
                    assert!(!machine.framebuffer_available() && !machine.frame_ready());
                    idle(&mut machine, 14);
                    assert!(!machine.vdp().interrupts().vertical_pending());
                    assert!(!machine.vdp().interrupts().horizontal_pending());
                    for kind in 0..4 {
                        let mut rejected = access(STATUS, ByteLanes::Both);
                        match kind {
                            0 => rejected.kind = AccessKind::Prefetch,
                            1 => rejected.lanes = ByteLanes::Lower,
                            2 => rejected.function_code = FunctionCode::CpuSpace,
                            _ => {
                                rejected.logical_address = 0xc00008;
                                rejected.bus_address = 0xc00008;
                            }
                        }
                        let before = machine.clone();
                        let response = transfer(&mut machine, rejected);
                        assert_eq!(
                            response.result,
                            Err(BusError::VdpPort(VdpPortError::Status(
                                VdpStatusError::UnknownBusHistory
                            )))
                        );
                        assert_eq!(response.wait_clocks, 0);
                        assert!(machine == before);
                    }
                }
            }
        }
    }
}

fn delayed_begin() {
    let mut machine = super::super::bank_reservation::ready(&[0x7e]);
    assert!(
        machine
            .sound_schedule
            .pending
            .as_ref()
            .unwrap()
            .reservation()
            .is_some()
    );
    let begin = machine.bus.now.get();
    let result = transfer(&mut machine, access(STATUS, ByteLanes::Both));
    assert!(result.result.is_ok());
    assert!(result.wait_clocks > 0);
    assert_eq!(
        machine.bus.now.get(),
        begin + (4 + u64::from(result.wait_clocks)) * 7
    );
    assert!(!machine.vdp().has_timed_status());
    machine.bus.validate_state().unwrap();
}

fn equal(left: &mut Emulator, right: &mut Emulator) {
    assert!(left == right);
    assert_eq!(left.encode_state().unwrap(), right.encode_state().unwrap());
    assert_eq!(
        left.encode_frontend_state().unwrap(),
        right.encode_frontend_state().unwrap()
    );
    let a = left.drain_audio_samples();
    let b = right.drain_audio_samples();
    assert!(a.iter().chain(&b).all(|sample| sample.is_finite()));
    assert_eq!(
        a.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        b.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
}

fn cpu_and_codec() {
    for (region, concurrent) in [(Region::NtscUs, false), (Region::Pal, true)] {
        let mut source = fresh(region, concurrent, true);
        source.step_field().unwrap();
        assert!(source.video_error().is_none());
        assert!(source.vdp().requires_timed_ports());
        assert_eq!(source.vdp().registers().registers()[1], Some(0x20));
        assert!(!source.framebuffer_available());
        let raw = source.encode_state().unwrap();
        let frontend = source.encode_frontend_state().unwrap();
        let mut raw_reference = source.clone();
        raw_reference.decode_state(&raw).unwrap();
        let mut raw_consumer = fresh(region, concurrent, true);
        raw_consumer.decode_state(&raw).unwrap();
        let mut frontend_consumer = fresh(region, concurrent, true);
        frontend_consumer.decode_frontend_state(&frontend).unwrap();
        equal(&mut source, &mut frontend_consumer);
        equal(&mut raw_reference, &mut raw_consumer);
        for _ in 0..2 {
            for machine in [
                &mut source,
                &mut frontend_consumer,
                &mut raw_reference,
                &mut raw_consumer,
            ] {
                machine.step_field().unwrap();
                assert!(machine.video_error().is_none());
                assert!(!machine.framebuffer_available() && !machine.frame_ready());
            }
            equal(&mut source, &mut frontend_consumer);
            equal(&mut raw_reference, &mut raw_consumer);
        }
        for machine in [
            &mut source,
            &mut frontend_consumer,
            &mut raw_reference,
            &mut raw_consumer,
        ] {
            machine.bus.ram[0] = 1;
        }
        for field in 0..2 {
            for machine in [
                &mut source,
                &mut frontend_consumer,
                &mut raw_reference,
                &mut raw_consumer,
            ] {
                machine.step_field().unwrap();
                assert_eq!(machine.vdp().registers().registers()[1], Some(4));
                assert!(machine.video_error().is_none());
                assert_eq!(machine.framebuffer_available(), field == 1);
                assert_eq!(machine.frame_ready(), field == 1);
            }
            equal(&mut source, &mut frontend_consumer);
            equal(&mut raw_reference, &mut raw_consumer);
            if field == 0 {
                let midway = source.encode_frontend_state().unwrap();
                frontend_consumer.decode_frontend_state(&midway).unwrap();
                let midway = raw_reference.encode_state().unwrap();
                raw_reference.decode_state(&midway).unwrap();
                raw_consumer.decode_state(&midway).unwrap();
                equal(&mut source, &mut frontend_consumer);
                equal(&mut raw_reference, &mut raw_consumer);
            }
        }
        let mut interrupted = source.clone();
        let begin = interrupted.bus.now;
        interrupted
            .bus
            .vdp
            .begin_timed_status(begin, MasterTicks::new(begin.get() + 28), region)
            .unwrap();
        let before = interrupted.clone();
        assert!(
            interrupted
                .encode_state()
                .unwrap_err()
                .to_string()
                .contains("interrupted VDP port transaction")
        );
        assert!(interrupted.encode_frontend_state().is_err());
        assert!(interrupted == before);
    }
}

pub(super) fn checks() {
    phased_reads();
    delayed_begin();
    cpu_and_codec();
    cpu_memory_and_codec();
    unknown_status_preflight();
}

fn memory_machine(region: Region, concurrent: bool) -> Emulator {
    let words: &[u16] = &[
        0x33fc, 0x8f02, 0x00c0, 0x0004, 0x33fc, 0x4000, 0x00c0, 0x0004, 0x33fc, 0x0000, 0x00c0,
        0x0004, 0x33fc, 0xcafe, 0x00c0, 0x0000, 0x4e71, 0x33fc, 0x8120, 0x00c0, 0x0004, 0x33fc,
        0x8c81, 0x00c0, 0x0004, 0x3039, 0x00c0, 0x0004, 0x4e71, 0x60fc,
    ];
    let mut rom = vec![0; 0x200];
    rom[..8].copy_from_slice(&[0, 0xff, 0x80, 0, 0, 0, 1, 0]);
    for (index, word) in words.iter().enumerate() {
        rom[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound: concurrent,
        },
    )
    .unwrap();
    machine.set_sample_rate(48_000);
    machine
}

fn cpu_memory_and_codec() {
    for region in [Region::NtscUs, Region::Pal] {
        for concurrent in [false, true] {
            let mut source = memory_machine(region, concurrent);
            for _ in 0..4 {
                source.step_instruction().unwrap();
            }
            assert_eq!(source.cpu.registers.pc, 0x120);
            assert!(!source.vdp().has_main_data());
            let raw = source.encode_state().unwrap();
            let front = source.encode_frontend_state().unwrap();
            let mut reference = source.clone();
            reference.decode_state(&raw).unwrap();
            let mut raw_target = memory_machine(region, concurrent);
            raw_target.decode_state(&raw).unwrap();
            let mut front_target = memory_machine(region, concurrent);
            front_target.decode_frontend_state(&front).unwrap();
            equal(&mut source, &mut front_target);
            equal(&mut reference, &mut raw_target);
            for _ in 0..64 {
                let expected = source.step_instruction().unwrap();
                assert_eq!(front_target.step_instruction().unwrap(), expected);
                let expected = reference.step_instruction().unwrap();
                assert_eq!(raw_target.step_instruction().unwrap(), expected);
                equal(&mut source, &mut front_target);
                equal(&mut reference, &mut raw_target);
            }
            assert!(source.vdp().requires_timed_ports());
            assert_eq!(source.vdp().registers().registers()[1], Some(0x20));
            assert_eq!(source.vdp().registers().registers()[12], Some(0));
            assert!(!source.framebuffer_available());
            assert!(source.video_error().is_none());
            let mut interrupted = memory_machine(region, concurrent);
            interrupted
                .bus
                .vdp
                .begin_main_data(interrupted.bus.now, true)
                .unwrap();
            let before = interrupted.clone();
            assert!(interrupted.encode_state().is_err());
            assert!(interrupted.encode_frontend_state().is_err());
            assert!(interrupted == before);
            for (address, wrong_clock) in [(0xc00000, true), (0xc00000, false), (STATUS, false)] {
                let start = zeff_m68k::timing::CpuClocks(
                    interrupted.bus.cpu_clock.0 + u64::from(wrong_clock),
                );
                let result = if concurrent {
                    ScheduledBus::new(
                        &mut interrupted.bus,
                        &mut interrupted.sound_cpu,
                        &mut interrupted.sound_schedule,
                    )
                    .read_cycle(access(address, ByteLanes::Both), start)
                } else {
                    interrupted
                        .bus
                        .read_cycle(access(address, ByteLanes::Both), start)
                };
                assert!(result.result.is_err());
                assert_eq!(result.wait_clocks, 0);
                assert!(interrupted == before);
            }
        }
    }
}

fn unknown_status_preflight() {
    for region in [Region::NtscUs, Region::Pal] {
        for concurrent in [false, true] {
            let mut machine = fresh(region, concurrent, false);
            control(&mut machine, 0x8144);
            idle(&mut machine, 4);
            control(&mut machine, 0x8120);
            assert!(machine.vdp().registers().pending().is_some());
            for phase in 0..2 {
                let before = machine.clone();
                assert!(
                    transfer(&mut machine, access(STATUS, ByteLanes::Both))
                        .result
                        .is_err()
                );
                assert!(machine == before);
                if phase == 0 {
                    idle(&mut machine, 4);
                    assert!(machine.vdp().requires_timed_ports());
                }
            }
        }
    }
    let mut reserved = super::super::bank_reservation::ready(&[0x7e, 0xc3, 3, 0]);
    assert_eq!(control_cycle(&mut reserved, 0x8144).result, Ok(()));
    idle(&mut reserved, 4);
    assert_eq!(control_cycle(&mut reserved, 0x8120).result, Ok(()));
    for _ in 0..100 {
        if reserved
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|p| p.reservation().is_some())
        {
            break;
        }
        idle(&mut reserved, 1);
    }
    assert!(
        reserved
            .sound_schedule
            .pending
            .as_ref()
            .unwrap()
            .reservation()
            .is_some()
    );
    let before = reserved.clone();
    let result = transfer(&mut reserved, access(STATUS, ByteLanes::Both));
    assert!(result.result.is_err());
    assert_eq!(result.wait_clocks, 0);
    assert!(reserved == before);
}
