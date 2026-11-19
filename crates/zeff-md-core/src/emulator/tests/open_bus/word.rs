use super::*;
use crate::hardware::bus::Prefetch;

#[path = "unused_cartridge.rs"]
mod unused_cartridge;

fn controls(machine: &mut Emulator, granted: bool) {
    machine.bus.sound.set_reset(!granted);
    machine.bus.sound.request(granted);
    machine.bus.sound.complete_boundary();
    assert_eq!(machine.sound_bus_granted(), granted);
}

fn emit(label: &str, hash: u64, cases: usize) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("{label} {hash:016x} cases={cases}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("{label} {hash:016x} cases={cases}");
}

fn record(hash: &mut u64, value: impl std::fmt::Debug) {
    for byte in format!("{value:?}").bytes() {
        *hash = (*hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn busreq_word_all_prefetched_bits_and_both_status_values() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for concurrent in [false, true] {
        for granted in [false, true] {
            let mut machine = super::super::machine(&[0x4e71; 2]);
            controls(&mut machine, granted);
            let sound = machine.bus.sound.clone();
            for source in 0..=u16::MAX {
                machine.bus.prefetch = Prefetch::Full(source);
                let value = read(
                    &mut machine,
                    concurrent,
                    access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Both),
                )
                .unwrap();
                assert_eq!(value & 0xfeff, source & 0xfeff);
                assert_eq!(value & 0x0100, u16::from(!granted) << 8);
                assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                record(&mut hash, value);
                cases += 1;
            }
            assert_eq!(machine.bus.sound, sound);
            assert!(machine.sound_schedule.fault.is_none());
        }
    }
    emit("busreq-word-bits-v1", hash, cases);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn busreq_word_aliases_require_full_provenance_without_control_effects() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    unused_cartridge::check(&mut hash, &mut cases);
    for concurrent in [false, true] {
        for granted in [false, true] {
            let mut machine = super::super::machine(&[0x4e71; 2]);
            controls(&mut machine, granted);
            let sound = machine.bus.sound.clone();
            for offset in (0..=0xfe).step_by(2) {
                let request = access(
                    SOUND_BUS_REQUEST + offset,
                    AccessKind::Data,
                    ByteLanes::Both,
                );
                for source in [0, 1, 0x0801, 0x4e71, 0xa55a, 0xffff] {
                    machine.bus.prefetch = Prefetch::Full(source);
                    assert_eq!(
                        read(&mut machine, concurrent, request),
                        Ok((source & 0xfeff) | (u16::from(!granted) << 8))
                    );
                    assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                    assert_eq!(
                        read(&mut machine, concurrent, request),
                        Err(BusError::UnsupportedAccess {
                            address: request.bus_address,
                            direction: zeff_m68k::bus::Direction::Read
                        })
                    );
                    cases += 1;
                }
                for source in [
                    Prefetch::Empty,
                    Prefetch::UpperOnly(0),
                    Prefetch::UpperOnly(0xff),
                ] {
                    machine.bus.prefetch = source;
                    let before = machine.clone();
                    assert_eq!(machine.peek(0x100), Some(0x4e));
                    assert_eq!(machine, before);
                    assert_eq!(
                        read(&mut machine, concurrent, request),
                        Err(BusError::UnsupportedAccess {
                            address: request.bus_address,
                            direction: zeff_m68k::bus::Direction::Read
                        })
                    );
                    assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                    record(&mut hash, (source, offset, granted));
                    cases += 1;
                }
                let mut lower = access(request.bus_address, AccessKind::Data, ByteLanes::Lower);
                lower.logical_address += 1;
                for (source, selected) in [
                    (Prefetch::Empty, None),
                    (Prefetch::UpperOnly(0xa5), None),
                    (Prefetch::Full(0x00a5), Some(0x00a5)),
                    (Prefetch::Full(0xff5a), Some(0x005a)),
                    (Prefetch::Full(0x66f6), Some(0x00f6)),
                    (Prefetch::Full(0x66f7), Some(0x00f7)),
                ] {
                    machine.bus.prefetch = source;
                    let expected = selected.ok_or(BusError::UnsupportedAccess {
                        address: request.bus_address,
                        direction: zeff_m68k::bus::Direction::Read,
                    });
                    assert_eq!(read(&mut machine, concurrent, lower), expected);
                    assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                    assert!(matches!(
                        read(&mut machine, concurrent, lower),
                        Err(BusError::UnsupportedAccess { .. })
                    ));
                    record(&mut hash, (source, offset, granted, expected));
                    cases += 1;
                }
                for kind in [
                    AccessKind::Opcode,
                    AccessKind::Extension,
                    AccessKind::Prefetch,
                ] {
                    machine.bus.prefetch = Prefetch::Full(0x66f7);
                    lower.kind = kind;
                    assert!(matches!(
                        read(&mut machine, concurrent, lower),
                        Err(BusError::UnsupportedAccess { .. })
                    ));
                    assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                }
                machine.bus.prefetch = Prefetch::UpperOnly(0xa5);
                let upper = access(request.bus_address, AccessKind::Data, ByteLanes::Upper);
                assert_eq!(
                    read(&mut machine, concurrent, upper),
                    Ok(0xa400 | (u16::from(!granted) << 8))
                );
                assert_eq!(
                    read(&mut machine, concurrent, upper),
                    Ok(u16::from(!granted) << 8)
                );
            }
            assert_eq!(machine.bus.sound, sound);
        }
    }
    lower_real_btst_continuations(&mut hash, &mut cases);
    lower_nonlocal_sound_invalidates(&mut hash, &mut cases);
    unused_input_words(&mut hash, &mut cases);
    unused_input_cmpi_continuations(&mut hash, &mut cases);
    emit("busreq-word-provenance-v1", hash, cases);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn busreq_word_real_fetch_order_clone_and_raw_continuation() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            for granted in [false, true] {
                for ram in [false, true] {
                    for next in [0x0800u16, 0x0801] {
                        let program = [
                            0x4e71, 0x3239, 0x00a1, 0x1100, next, 0, 0x67f4, 0x4e71, 0x4e71,
                        ];
                        let mut image = vec![0; 0x200];
                        image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
                        let entry: &[u16] = if ram {
                            &[0x4ef9, 0x00ff, 0x0100, 0x4e71]
                        } else {
                            &program
                        };
                        for (index, word) in entry.iter().enumerate() {
                            image[0x100 + index * 2..0x102 + index * 2]
                                .copy_from_slice(&word.to_be_bytes());
                        }
                        let mut machine = Emulator::new(
                            image,
                            LoadConfig {
                                region,
                                concurrent_sound,
                            },
                        )
                        .unwrap();
                        controls(&mut machine, granted);
                        if ram {
                            for (index, word) in program.iter().enumerate() {
                                machine.bus.ram[0x100 + index * 2..0x102 + index * 2]
                                    .copy_from_slice(&word.to_be_bytes());
                            }
                            machine.step_instruction().unwrap();
                        }
                        machine.step_instruction().unwrap();
                        assert_eq!(machine.queued_words(), Some([0x3239, 0x00a1]));
                        machine.cpu.registers.d[1] = 0x12340000;
                        let saved = machine.encode_state().unwrap();
                        assert_eq!(
                            u32::from_le_bytes(saved[8..12].try_into().unwrap()),
                            crate::save_state::STATE_VERSION
                        );
                        let mut cloned = machine.clone();
                        let mut restored = machine.clone();
                        restored.decode_state(&saved).unwrap();
                        assert_eq!(machine.cpu, restored.cpu);
                        assert_eq!(machine.bus.prefetch, restored.bus.prefetch);
                        assert_eq!(saved, restored.encode_state().unwrap());
                        let pc = machine.registers().pc;
                        let clock = machine.cpu_clock();
                        for current in [&mut machine, &mut cloned, &mut restored] {
                            let step = current.step_instruction().unwrap();
                            assert_eq!(step.elapsed.get(), 112);
                            assert_eq!(current.cpu_clock().0 - clock.0, 16);
                            assert_eq!(
                                current.registers().d[1],
                                0x12340000
                                    | u32::from((next & 0xfeff) | (u16::from(!granted) << 8))
                            );
                            assert_eq!(current.registers().pc, pc + 6);
                            assert_eq!(current.queued_words(), Some([next, 0]));
                            assert_eq!(current.sound_bus_granted(), granted);
                            if next == 0x0801 {
                                current.step_instruction().unwrap();
                                assert_eq!(current.registers().status_register() & 4, 0);
                                current.step_instruction().unwrap();
                                assert_eq!(current.registers().pc, pc + 12);
                            }
                        }
                        assert_eq!(machine, cloned);
                        assert_eq!(machine.cpu, restored.cpu);
                        assert_eq!(machine.bus.prefetch, restored.bus.prefetch);
                        assert_eq!(
                            machine.encode_state().unwrap(),
                            restored.encode_state().unwrap()
                        );
                        record(
                            &mut hash,
                            (
                                region,
                                concurrent_sound,
                                granted,
                                ram,
                                next,
                                machine.cpu,
                                machine.bus.prefetch,
                            ),
                        );
                        cases += 1;
                    }
                }
            }
        }
    }
    emit("busreq-word-real-v1", hash, cases);
}

fn lower_real_btst_continuations(hash: &mut u64, cases: &mut usize) {
    #[cfg(feature = "first-failure-diagnostic")]
    use crate::diagnostic::{Callback, Capture, Observation, Outcome};

    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            for state in 0..3 {
                for next in [0x66f6u16, 0x66f7] {
                    let program = [0x4e71, 0x0839, 0, 0x00a1, 0x1101, next, 0x4e71, 0x4e71];
                    let mut image = vec![0; 0x200];
                    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
                    for (index, word) in program.iter().enumerate() {
                        image[0x100 + index * 2..0x102 + index * 2]
                            .copy_from_slice(&word.to_be_bytes());
                    }
                    let mut machine = Emulator::new(
                        image,
                        LoadConfig {
                            region,
                            concurrent_sound,
                        },
                    )
                    .unwrap();
                    machine.step_instruction().unwrap();
                    controls(&mut machine, state == 1);
                    if state == 2 {
                        machine.bus.sound.set_reset(false);
                        machine.bus.sound.ram[..2].copy_from_slice(&[0, 0x76]);
                    }
                    assert_eq!(machine.queued_words(), Some([0x0839, 0]));
                    let before = (
                        machine.sound_bus_requested(),
                        machine.sound_bus_granted(),
                        machine.sound_reset_asserted(),
                    );
                    let saved = machine.encode_state().unwrap();
                    assert_eq!(
                        u32::from_le_bytes(saved[8..12].try_into().unwrap()),
                        crate::save_state::STATE_VERSION,
                    );
                    let mut cloned = machine.clone();
                    let mut restored = machine.clone();
                    restored.decode_state(&saved).unwrap();
                    let pc = machine.registers().pc;
                    let clock = machine.cpu_clock();
                    for current in [&mut machine, &mut cloned, &mut restored] {
                        let registers = current.registers();
                        #[cfg(feature = "first-failure-diagnostic")]
                        let capture = Capture::begin().unwrap();
                        current.step_instruction().unwrap();
                        #[cfg(feature = "first-failure-diagnostic")]
                        {
                            let report = capture.finish(current).unwrap();
                            assert!(report.complete);
                            let records: Vec<_> = report.history.iter().flatten().collect();
                            let index = records
                                .iter()
                                .position(|record| {
                                    record.callback == Callback::Read
                                        && record.access.is_some_and(|access| {
                                            access.bus_address == SOUND_BUS_REQUEST
                                        })
                                })
                                .unwrap();
                            let read = records[index];
                            let mut expected =
                                access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Lower);
                            expected.logical_address += 1;
                            assert_eq!(read.access, Some(expected));
                            assert_eq!(read.service.unwrap().prefetch, Observation::Full(next));
                            assert_eq!(read.outcome, Some(Outcome::Read(next & 0xff)));
                            assert_eq!(read.wait_clocks, 0);
                            let fetched: Vec<_> = records[..index]
                                .iter()
                                .filter_map(|record| {
                                    record
                                        .access
                                        .filter(|access| access.kind == AccessKind::Prefetch)
                                        .map(|_| record.outcome)
                                })
                                .collect();
                            assert_eq!(
                                fetched,
                                vec![
                                    Some(Outcome::Read(0x00a1)),
                                    Some(Outcome::Read(0x1101)),
                                    Some(Outcome::Read(next)),
                                ]
                            );
                            assert_eq!(read.returned.unwrap().bus.prefetch, Observation::Empty);
                        }
                        assert_eq!(current.cpu_clock().0 - clock.0, 20);
                        assert_eq!(current.registers().pc, pc + 8);
                        assert_eq!(current.queued_words(), Some([next, 0x4e71]));
                        assert_eq!(current.registers().d, registers.d);
                        assert_eq!(current.registers().a, registers.a);
                        let expected_sr =
                            (registers.status_register() & !4) | if next == 0x66f6 { 4 } else { 0 };
                        assert_eq!(current.registers().status_register(), expected_sr);
                        assert_eq!(
                            (
                                current.sound_bus_requested(),
                                current.sound_bus_granted(),
                                current.sound_reset_asserted(),
                            ),
                            before
                        );
                        if next == 0x66f6 {
                            current.step_instruction().unwrap();
                            assert_eq!(current.registers().pc, pc + 10);
                        }
                    }
                    assert_eq!(machine, cloned);
                    assert_eq!(
                        machine.encode_state().unwrap(),
                        restored.encode_state().unwrap()
                    );
                    record(hash, (region, concurrent_sound, state, next, machine.cpu));
                    *cases += 1;
                }
            }
        }
    }
}

fn lower_nonlocal_sound_invalidates(hash: &mut u64, cases: &mut usize) {
    let mut machine = super::super::machine(&[0x4e71; 2]);
    machine.bus.sound.set_reset(false);
    machine.bus.sound.ram[..4].copy_from_slice(&[0x3a, 0, 0x80, 0x76]);
    let pending = PendingSound::prepare(
        &machine.sound_cpu,
        &machine.bus.sound,
        &machine.bus.cartridge,
        machine.bus.now.get(),
    )
    .unwrap();
    assert!(pending.any_bank_access);
    assert!(pending.bank_access.is_none());
    assert!(pending.reservation().is_none());
    machine.sound_schedule.pending = Some(pending);
    machine.bus.sound.request(true);
    machine.bus.prefetch = Prefetch::Full(0x66f7);
    let mut request = access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Lower);
    request.logical_address += 1;
    assert!(matches!(
        read(&mut machine, true, request),
        Err(BusError::UnsupportedAccess { .. })
    ));
    assert_eq!(machine.bus.prefetch, Prefetch::Empty);
    record(
        hash,
        machine
            .sound_schedule
            .pending
            .as_ref()
            .unwrap()
            .any_bank_access,
    );
    *cases += 1;
}

fn unused_input_words(hash: &mut u64, cases: &mut usize) {
    use crate::constants::{IO_UNUSED_END, IO_UNUSED_START};
    for concurrent in [false, true] {
        for granted in [false, true] {
            let mut machine = super::super::machine(&[0x4e71; 2]);
            controls(&mut machine, granted);
            let input = machine.bus.input.clone();
            let sound = machine.bus.sound.clone();
            for address in (IO_UNUSED_START..=IO_UNUSED_END).step_by(2) {
                for alias in [0, 0x01000000, 0xff000000] {
                    let mut request = access(address, AccessKind::Data, ByteLanes::Both);
                    request.logical_address |= alias;
                    for function in [FunctionCode::UserData, FunctionCode::SupervisorData] {
                        request.function_code = function;
                        for word in [0x12e7, 0x1364, 0xa45a, 0xa55a] {
                            machine.bus.prefetch = Prefetch::Full(word);
                            assert_eq!(machine.peek(request.logical_address), None);
                            assert_eq!(machine.bus.prefetch, Prefetch::Full(word));
                            assert_eq!(read(&mut machine, concurrent, request), Ok(word));
                            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                            assert!(matches!(
                                read(&mut machine, concurrent, request),
                                Err(BusError::UnsupportedAccess { .. })
                            ));
                            record(hash, (address, alias, function, granted, word));
                            *cases += 1;
                        }
                    }
                }
            }
            let request = access(IO_UNUSED_END, AccessKind::Data, ByteLanes::Both);
            for tag in [Prefetch::Empty, Prefetch::UpperOnly(0xa5)] {
                machine.bus.prefetch = tag;
                assert!(matches!(
                    read(&mut machine, concurrent, request),
                    Err(BusError::UnsupportedAccess { .. })
                ));
                assert_eq!(machine.bus.prefetch, Prefetch::Empty);
            }
            let mut rejected = Vec::new();
            for lanes in [ByteLanes::Upper, ByteLanes::Lower] {
                let mut other = request;
                other.lanes = lanes;
                if lanes == ByteLanes::Lower {
                    other.logical_address += 1;
                }
                rejected.push(other);
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
                let mut other = request;
                other.kind = kind;
                rejected.push(other);
            }
            for function in [
                FunctionCode::UserProgram,
                FunctionCode::SupervisorProgram,
                FunctionCode::CpuSpace,
            ] {
                let mut other = request;
                other.function_code = function;
                rejected.push(other);
            }
            for (bus_address, logical_address) in [
                (IO_UNUSED_START + 1, IO_UNUSED_START + 1),
                (IO_UNUSED_END, IO_UNUSED_END - 2),
                (IO_UNUSED_END, IO_UNUSED_END + 1),
                (0xa10100, 0xa10100),
                (0x01a10020, 0x01a10020),
            ] {
                let mut other = request;
                other.bus_address = bus_address;
                other.logical_address = logical_address;
                rejected.push(other);
            }
            for other in rejected {
                machine.bus.prefetch = Prefetch::Full(0xa55a);
                assert!(matches!(
                    read(&mut machine, concurrent, other),
                    Err(BusError::UnsupportedAccess { .. })
                ));
                assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                record(hash, other);
                *cases += 1;
            }
            let neighbor = access(IO_UNUSED_START - 2, AccessKind::Data, ByteLanes::Both);
            let mut empty = machine.clone();
            empty.bus.prefetch = Prefetch::Empty;
            machine.bus.prefetch = Prefetch::Full(0xa55a);
            assert_eq!(
                read(&mut machine, concurrent, neighbor),
                read(&mut empty, concurrent, neighbor)
            );
            assert_eq!(machine.bus.input, input);
            assert_eq!(machine.bus.sound, sound);
        }
    }
}

fn unused_input_cmpi_continuations(hash: &mut u64, cases: &mut usize) {
    #[cfg(feature = "first-failure-diagnostic")]
    use crate::diagnostic::{Callback, Capture, Observation, Outcome};
    for concurrent_sound in [false, true] {
        for active in [false, true] {
            for next in [0x672au16, 0x6620, 0x472a] {
                let program = [0x4e71, 0x0c79, 0x57a8, 0x00a1, 0x00fe, next, 0x4e71, 0x4e71];
                let mut image = vec![0; 0x200];
                image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
                for (index, word) in program.iter().enumerate() {
                    image[0x100 + index * 2..0x102 + index * 2]
                        .copy_from_slice(&word.to_be_bytes());
                }
                let config = LoadConfig {
                    region: Region::NtscUs,
                    concurrent_sound,
                };
                let mut machine = Emulator::new(image.clone(), config).unwrap();
                machine.step_instruction().unwrap();
                if active {
                    machine.bus.sound.set_reset(false);
                    machine.bus.sound.ram[..2].copy_from_slice(&[0, 0x76]);
                }
                machine.bus.prefetch = Prefetch::Full(0x57a8);
                machine.cpu.registers.set_status_register(0x271f);
                assert_eq!(machine.queued_words(), Some([0x0c79, 0x57a8]));
                let raw = machine.encode_state().unwrap();
                let front = machine.encode_frontend_state().unwrap();
                assert_eq!(
                    u32::from_le_bytes(raw[8..12].try_into().unwrap()),
                    crate::save_state::STATE_VERSION
                );
                assert_eq!(u32::from_le_bytes(front[8..12].try_into().unwrap()), 1);
                let mut cloned = machine.clone();
                let mut restored = Emulator::new(image.clone(), config).unwrap();
                restored.decode_state(&raw).unwrap();
                assert_eq!(restored.encode_state().unwrap(), raw);
                let mut frontend = Emulator::new(image, config).unwrap();
                frontend.decode_frontend_state(&front).unwrap();
                assert_eq!(frontend.encode_frontend_state().unwrap(), front);
                for current in [&mut machine, &mut cloned, &mut restored, &mut frontend] {
                    let before = current.registers();
                    let input = current.bus.input.clone();
                    let controls = (
                        current.sound_bus_requested(),
                        current.sound_bus_granted(),
                        current.sound_reset_asserted(),
                    );
                    let clock = current.cpu_clock();
                    #[cfg(feature = "first-failure-diagnostic")]
                    let capture = Capture::begin().unwrap();
                    current.step_instruction().unwrap();
                    #[cfg(feature = "first-failure-diagnostic")]
                    {
                        let report = capture.finish(current).unwrap();
                        assert!(report.complete);
                        assert_eq!(report.overwritten, 0);
                        let records: Vec<_> = report
                            .history
                            .iter()
                            .flatten()
                            .filter(|r| r.callback == Callback::Read)
                            .collect();
                        let index = records
                            .iter()
                            .position(|r| r.access.is_some_and(|a| a.bus_address == 0xa100fe))
                            .unwrap();
                        let read = records[index];
                        let fetched: Vec<_> = records[..index]
                            .iter()
                            .map(|r| {
                                let access = r.access.unwrap();
                                assert_eq!(access.kind, AccessKind::Prefetch);
                                (access.logical_address, r.outcome)
                            })
                            .collect();
                        assert_eq!(
                            fetched,
                            vec![
                                (before.pc + 4, Some(Outcome::Read(0x00a1))),
                                (before.pc + 6, Some(Outcome::Read(0x00fe))),
                                (before.pc + 8, Some(Outcome::Read(next)))
                            ]
                        );
                        assert_eq!(
                            read.access,
                            Some(access(0xa100fe, AccessKind::Data, ByteLanes::Both))
                        );
                        assert_eq!(read.service.unwrap().prefetch, Observation::Full(next));
                        assert_eq!(read.outcome, Some(Outcome::Read(next)));
                        assert_eq!(read.returned.unwrap().bus.prefetch, Observation::Empty);
                        assert_eq!(read.wait_clocks, 0);
                        assert_eq!(
                            records[index + 1].access.unwrap().logical_address,
                            before.pc + 10
                        );
                        assert_eq!(
                            records[index + 1].access.unwrap().kind,
                            AccessKind::Prefetch
                        );
                        assert_eq!(records[index + 1].outcome, Some(Outcome::Read(0x4e71)));
                    }
                    assert_eq!(current.cpu_clock().0 - clock.0, 20);
                    assert_eq!(current.registers().pc, before.pc + 8);
                    assert_eq!(current.queued_words(), Some([next, 0x4e71]));
                    assert_eq!(current.registers().d, before.d);
                    assert_eq!(current.registers().a, before.a);
                    assert_eq!(
                        current.registers().status_register(),
                        (before.status_register() & !0xf) | if next < 0x57a8 { 9 } else { 0 }
                    );
                    assert_eq!(current.bus.input, input);
                    assert_eq!(
                        (
                            current.sound_bus_requested(),
                            current.sound_bus_granted(),
                            current.sound_reset_asserted()
                        ),
                        controls
                    );
                }
                assert_eq!(machine, cloned);
                assert_eq!(
                    machine.encode_state().unwrap(),
                    restored.encode_state().unwrap()
                );
                assert_eq!(
                    machine.encode_state().unwrap(),
                    frontend.encode_state().unwrap()
                );
                record(hash, (concurrent_sound, active, next, machine.cpu));
                *cases += 1;
            }
        }
    }
}
