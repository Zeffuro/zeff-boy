use super::device::{Device, Event};
use super::*;
use crate::bus::{AccessKind, FunctionCode};
use crate::timing::physical::{
    EntryFault, EntryOutcome, ExternalResetBoundary, Group0Capture, ImportedGroup0Boundary,
    ProcessingClass, ResetQualification, SswProfile,
};
use crate::timing::{CpuClocks, TimedOutcome};
use crate::{Registers, SUPERVISOR};
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn cpu(origin: u8, clock: u64) -> (PrefetchCpu, Device) {
    let mut registers = Registers::new();
    registers.pc = 0x100;
    registers.ssp = 0xff1000;
    registers.usp = 0xfedcba98;
    for (i, value) in registers.d.iter_mut().chain(&mut registers.a).enumerate() {
        *value = 0xa5030000 + i as u32 * 0x105;
    }
    registers.set_status_register(
        0x15 | if origin & 1 != 0 || origin == 4 {
            SUPERVISOR
        } else {
            0
        } | if origin == 2 || origin == 3 { TRACE } else { 0 },
    );
    let words = if origin == 4 {
        [0x4e72, 0x2700]
    } else {
        [0x4e71, 0x4e71]
    };
    let mut cpu = PrefetchCpu::from_prefetched(registers, words, CpuClocks(clock)).unwrap();
    let mut device = Device::new(clock, 0);
    if origin >= 2 {
        cpu.step_instruction(&mut device).unwrap();
        assert_eq!(cpu.stopped, origin == 4);
        assert_eq!(cpu.pending_trace, origin == 2 || origin == 3);
    }
    device.events.clear();
    (cpu, device)
}

fn reset(
    cpu: &mut PrefetchCpu,
    device: &mut Device,
) -> Result<crate::timing::physical::EntryStep, EntryFault<u8>> {
    cpu.external_warm_reset(
        device,
        ExternalResetBoundary {
            sampled_release: cpu.clock(),
            qualification: ResetQualification::QualifiedSafeAssertionAndRelease,
        },
    )
}

fn bytes(cpu: &PrefetchCpu) -> Vec<u8> {
    let mut writer = StateWriter::new();
    cpu.write_boundary_state(&mut writer).unwrap();
    writer.into_bytes()
}

fn record(hash: &mut u64, value: impl std::fmt::Debug) {
    for byte in format!("{value:?}").bytes() {
        *hash = (*hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
}

fn emit(label: &str, hash: u64, cases: usize) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("{label} {hash:016x} cases={cases}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("{label} {hash:016x} cases={cases}");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_boundary_ready_query_preserves_stop_trace_and_rejects_stale_prefetch() {
    let mut fixtures: Vec<_> = (0..5).map(|origin| cpu(origin, 123456)).collect();
    let (seed, mut device) = cpu(0, 123456);
    let mut registers = seed.registers;
    registers.set_status_register(0xa700);
    let mut traced_stop =
        PrefetchCpu::from_prefetched(registers, [0x4e72, 0xa700], seed.clock()).unwrap();
    traced_stop.step_instruction(&mut device).unwrap();
    assert!(traced_stop.stopped && traced_stop.pending_trace);
    fixtures.push((traced_stop, device));
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for (mut actual, device) in fixtures {
        let original = actual.clone();
        for edit in [0, 2, 1] {
            actual = original.clone();
            actual.registers.pc = actual.registers.pc.wrapping_add(edit);
            let before = actual.clone();
            let expected = if edit == 1 || edit == 2 && !actual.stopped {
                Err(ExternalResetBoundaryError::NotOrdinaryBoundary)
            } else {
                Ok(())
            };
            assert_eq!(actual.check_external_reset_boundary(), Ok(()));
            assert_eq!(actual.check_external_reset_ready_boundary(), expected);
            assert_eq!(actual.check_external_reset_ready_boundary(), expected);
            assert_eq!(actual, before);
            record(&mut hash, (edit, actual, expected, &device));
            cases += 1;
        }
    }
    let (ordinary, _) = cpu(0, 0);
    let (stopped, _) = cpu(4, 0);
    for kind in 0..5 {
        let mut actual = match kind {
            0 => PrefetchCpu::default(),
            2 => stopped.clone(),
            _ => ordinary.clone(),
        };
        match kind {
            1 => actual.stopped = true,
            2 => actual.stopped = false,
            3 => actual.frozen = true,
            4 => actual.physical_state = PhysicalEntryState::Critical(EntryKind::ExternalWarmReset),
            _ => {}
        }
        let expected = if kind == 3 {
            ExternalResetBoundaryError::HostFrozen
        } else {
            ExternalResetBoundaryError::NotOrdinaryBoundary
        };
        let before = actual.clone();
        assert_eq!(actual.check_external_reset_ready_boundary(), Err(expected));
        assert_eq!(actual, before);
        if kind < 3 {
            assert_eq!(actual.check_external_reset_boundary(), Ok(()));
        }
        record(&mut hash, (kind, actual, expected));
        cases += 1;
    }
    emit("reset-boundary-ready-v1", hash, cases);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_boundary_success_codec_irq_continuation() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for origin in 0..5 {
        for waits in [0, 3] {
            for level in [0, 7] {
                for new_edge in [false, true] {
                    let (mut cpu, mut device) = cpu(origin, 123456);
                    device.waits = waits;
                    assert!(cpu.interrupt_input().set_level(level));
                    let before = cpu.clone();
                    assert_eq!(cpu.check_external_reset_boundary(), Ok(()));
                    assert_eq!(cpu, before);
                    let step = reset(&mut cpu, &mut device).unwrap();
                    assert_eq!(step.kind, EntryKind::ExternalWarmReset);
                    assert_eq!(step.clocks, 40 + 6 * u64::from(waits));
                    assert_eq!(cpu.clock().0, device.clock);
                    assert_eq!(cpu.registers.d, before.registers.d);
                    assert_eq!(cpu.registers.a, before.registers.a);
                    assert_eq!(cpu.registers.usp, before.registers.usp);
                    assert_eq!(cpu.registers.ssp, 0xff2dc0);
                    assert_eq!(cpu.registers.pc, 0x300);
                    assert_eq!(
                        cpu.registers.status_register(),
                        0x2700 | (before.registers.status_register() & 31)
                    );
                    assert_eq!(cpu.queued_words(), Some([0x4e71, 0x4e71]));
                    let reads: Vec<_> = device
                        .events
                        .iter()
                        .filter_map(|event| match event {
                            Event::Read(access, _) => {
                                Some((access.logical_address, access.function_code, access.kind))
                            }
                            _ => None,
                        })
                        .collect();
                    assert_eq!(
                        reads,
                        [0, 2, 4, 6, 0x300, 0x302]
                            .into_iter()
                            .enumerate()
                            .map(|(i, a)| (
                                a,
                                FunctionCode::SupervisorProgram,
                                if i < 4 {
                                    AccessKind::ResetVector
                                } else {
                                    AccessKind::Prefetch
                                }
                            ))
                            .collect::<Vec<_>>()
                    );
                    assert_eq!(
                        device
                            .events
                            .iter()
                            .filter_map(|event| match event {
                                Event::Idle(_, clocks) => Some(*clocks),
                                _ => None,
                            })
                            .collect::<Vec<_>>(),
                        [14, 2]
                    );
                    assert_eq!(device.events.len(), 8);
                    assert!(cpu.write_boundary_state(&mut StateWriter::new()).is_err());
                    if new_edge {
                        cpu.interrupt_input().set_level(0);
                        cpu.interrupt_input().set_level(7);
                    }
                    let mut expected = cpu.clone();
                    expected.physical_state = PhysicalEntryState::Idle;
                    let bus_before = device.clone();
                    assert_eq!(cpu.finish_external_reset_boundary(), Ok(()));
                    assert_eq!(cpu, expected);
                    assert_eq!(device, bus_before);
                    assert_eq!(cpu.check_external_reset_boundary(), Ok(()));
                    let saved = bytes(&cpu);
                    let mut restored = PrefetchCpu::default();
                    let mut reader = StateReader::new(&saved);
                    restored.read_boundary_state(&mut reader).unwrap();
                    assert!(reader.is_exhausted());
                    assert_eq!(restored, cpu);
                    assert_eq!(bytes(&restored), saved);
                    let unchanged = cpu.clone();
                    assert_eq!(
                        cpu.finish_external_reset_boundary(),
                        Err(ExternalResetBoundaryError::IncompleteReset)
                    );
                    assert_eq!(cpu, unchanged);
                    let mut copy = device.clone();
                    for index in 0..2 {
                        let outcome = cpu.step_boundary(&mut device).unwrap();
                        assert_eq!(outcome, restored.step_boundary(&mut copy).unwrap());
                        if index == 0 {
                            assert_eq!(
                                matches!(
                                    outcome,
                                    TimedOutcome::Interrupt {
                                        level: 7,
                                        vector: 31,
                                        ..
                                    }
                                ),
                                new_edge
                            );
                        }
                        assert_eq!(cpu, restored);
                        assert_eq!(device, copy);
                    }
                    record(&mut hash, (saved, cpu, device));
                    cases += 1;
                }
            }
        }
    }
    emit("reset-boundary-success-v1", hash, cases);
}

fn rejected(cpu: &mut PrefetchCpu, device: &Device, error: ExternalResetBoundaryError) {
    let before = cpu.clone();
    let bus = device.clone();
    assert_eq!(cpu.finish_external_reset_boundary(), Err(error));
    assert_eq!(*cpu, before);
    assert_eq!(*device, bus);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_boundary_failed_entries_cannot_finalize() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for at in 1..=6 {
        for outcome in [
            EntryOutcome::HostFailure(73),
            EntryOutcome::CriticalAbortReleased,
            EntryOutcome::TimingUnavailable,
            EntryOutcome::Complete(()),
        ] {
            for waits in [0, 3] {
                let (mut cpu, mut device) = cpu(1, 900);
                device.waits = waits;
                device.injection = Some((at, outcome.clone()));
                assert!(reset(&mut cpu, &mut device).is_err());
                assert_eq!(device.reads, at);
                assert!(cpu.queued_words().is_none());
                let frozen = matches!(
                    outcome,
                    EntryOutcome::HostFailure(_) | EntryOutcome::Complete(())
                );
                rejected(
                    &mut cpu,
                    &device,
                    if frozen {
                        ExternalResetBoundaryError::HostFrozen
                    } else {
                        ExternalResetBoundaryError::IncompleteReset
                    },
                );
                let before = cpu.clone();
                let bus_before = device.clone();
                assert_eq!(
                    cpu.check_external_reset_boundary(),
                    Err(if frozen {
                        ExternalResetBoundaryError::HostFrozen
                    } else {
                        ExternalResetBoundaryError::NotOrdinaryBoundary
                    })
                );
                assert_eq!(cpu, before);
                assert_eq!(device, bus_before);
                device.injection = None;
                device.reads = 0;
                let result = reset(&mut cpu, &mut device);
                if frozen {
                    assert_eq!(result, Err(EntryFault::HostFrozen));
                    assert_eq!(cpu, before);
                    assert_eq!(device.events, bus_before.events);
                } else {
                    assert!(result.is_ok());
                    assert_eq!(cpu.finish_external_reset_boundary(), Ok(()));
                }
                record(&mut hash, (cpu, device));
                cases += 1;
            }
        }
    }
    let (mut cpu, mut device) = cpu(1, 900);
    device.words.insert(6, 0x301);
    assert!(reset(&mut cpu, &mut device).is_err());
    assert!(matches!(device.events.last(), Some(Event::Detect(..))));
    rejected(
        &mut cpu,
        &device,
        ExternalResetBoundaryError::IncompleteReset,
    );
    record(&mut hash, (cpu, device));
    cases += 1;
    emit("reset-boundary-failure-v1", hash, cases);
}

fn imported() -> ImportedGroup0Boundary {
    let mut registers = Registers::new();
    registers.pc = 0x100;
    registers.ssp = 0xff1000;
    ImportedGroup0Boundary {
        registers,
        clock: CpuClocks(100),
        capture: Group0Capture {
            vector: 2,
            internal_pc: 0x100,
            phase_sr: 0x2700,
            ird: 0x4e71,
            logical_address: 0x400000,
            function_code: FunctionCode::SupervisorData,
            direction: crate::bus::Direction::Read,
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        },
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_boundary_admission_and_defensive_guards() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    let (mut completed, mut device) = cpu(1, 100);
    reset(&mut completed, &mut device).unwrap();
    for index in 0..12 {
        let mut changed = completed.clone();
        match index {
            0 => changed.physical_state = PhysicalEntryState::Idle,
            1 => changed.physical_state = PhysicalEntryState::Critical(EntryKind::ImportedGroup0),
            2 => changed.pipeline = None,
            3 => changed.pipeline.as_mut().unwrap().address += 2,
            4 => changed.registers.pc |= 1,
            5 => changed.physical_inspection = PhysicalInspection::Unavailable,
            6 => changed.imported_boundary = Some(imported()),
            7 => changed.stopped = true,
            8 => changed.pending_trace = true,
            9 => changed.registers.set_status_register(0x0700),
            10 => changed.registers.set_status_register(0xa700),
            11 => changed.registers.set_status_register(0x2000),
            _ => unreachable!(),
        }
        rejected(
            &mut changed,
            &device,
            ExternalResetBoundaryError::IncompleteReset,
        );
        record(&mut hash, changed);
        cases += 1;
    }
    let mut pending = PrefetchCpu::from_group0_boundary(imported()).unwrap();
    let before = pending.clone();
    assert_eq!(
        pending.check_external_reset_boundary(),
        Err(ExternalResetBoundaryError::NotOrdinaryBoundary)
    );
    assert_eq!(pending, before);
    rejected(
        &mut pending,
        &device,
        ExternalResetBoundaryError::IncompleteReset,
    );
    record(&mut hash, pending);
    cases += 1;
    let mut foreign = PrefetchCpu::from_group0_boundary(imported()).unwrap();
    let mut foreign_bus = Device::new(100, 0);
    foreign_bus.words.insert(8, 0);
    foreign_bus.words.insert(10, 0x300);
    foreign.enter_imported_group0(&mut foreign_bus).unwrap();
    rejected(
        &mut foreign,
        &foreign_bus,
        ExternalResetBoundaryError::IncompleteReset,
    );
    record(&mut hash, (foreign, foreign_bus));
    cases += 1;
    for index in 0..3 {
        let (mut ordinary, bus) = cpu(1, 200);
        match index {
            0 => ordinary.frozen = true,
            1 => ordinary.physical_inspection = PhysicalInspection::Unavailable,
            2 => ordinary.physical_state = PhysicalEntryState::Critical(EntryKind::ImportedGroup0),
            _ => unreachable!(),
        }
        let before = ordinary.clone();
        assert_eq!(
            ordinary.check_external_reset_boundary(),
            Err(if index == 0 {
                ExternalResetBoundaryError::HostFrozen
            } else {
                ExternalResetBoundaryError::NotOrdinaryBoundary
            })
        );
        assert_eq!(ordinary, before);
        record(&mut hash, (ordinary, bus));
        cases += 1;
    }
    for clock in [u64::MAX - 13, u64::MAX - u64::from(u32::MAX) - 13] {
        let (mut cpu, mut device) = cpu(1, clock);
        assert_eq!(reset(&mut cpu, &mut device), Err(EntryFault::ClockOverflow));
        rejected(&mut cpu, &device, ExternalResetBoundaryError::HostFrozen);
        let before = cpu.clone();
        assert_eq!(
            cpu.check_external_reset_boundary(),
            Err(ExternalResetBoundaryError::HostFrozen)
        );
        assert_eq!(cpu, before);
        record(&mut hash, (cpu, device));
        cases += 1;
    }
    let (mut cpu, mut device) = cpu(1, 200);
    let before = cpu.clone();
    let bus = device.clone();
    assert_eq!(
        cpu.external_warm_reset(
            &mut device,
            ExternalResetBoundary {
                sampled_release: cpu.clock(),
                qualification: ResetQualification::Unqualified
            }
        ),
        Err(EntryFault::Input(
            crate::timing::physical::EntryInputError::UnqualifiedReset
        ))
    );
    assert_eq!(cpu, before);
    assert_eq!(device, bus);
    assert_eq!(cpu.check_external_reset_boundary(), Ok(()));
    rejected(
        &mut cpu,
        &device,
        ExternalResetBoundaryError::IncompleteReset,
    );
    record(&mut hash, cpu);
    cases += 1;
    emit("reset-boundary-guards-v1", hash, cases);
}
