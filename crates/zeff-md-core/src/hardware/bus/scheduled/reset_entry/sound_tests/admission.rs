use super::super::admission::{AdmissionError, admit};
use super::*;
use crate::hardware::audio::trace::{main_origin, trace_chip};
use crate::hardware::bus::rmw::Phase;
use zeff_emu_common::audio_trace::AudioTraceTiming;
use zeff_m68k::timing::reset_boundary::ExternalResetBoundaryError;

const QUALIFIED: ResetQualification = ResetQualification::QualifiedSafeAssertionAndRelease;

fn unchanged(
    cpu: &PrefetchCpu,
    rig: &Rig,
    qualification: ResetQualification,
    expected: Result<CpuClocks, AdmissionError>,
) {
    let before = (cpu.clone(), rig.clone());
    let debug = format!("{cpu:?}, {rig:?}");
    for _ in 0..2 {
        assert_eq!(admit(cpu, &rig.bus, &rig.schedule, qualification), expected);
        assert_eq!((cpu, rig), (&before.0, &before.1));
        assert_eq!(format!("{cpu:?}, {rig:?}"), debug);
    }
}

fn rmw(rig: &mut Rig, phase: u8) {
    let access = BusAccess {
        logical_address: 0xff8000,
        bus_address: 0xff8000,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes: ByteLanes::Upper,
    };
    let clock = rig.bus.cpu_clock;
    assert_eq!(
        rig.bus.begin_rmw(access, clock),
        Ok(BusResponse::Complete(()))
    );
    if phase > 0 {
        assert!(rig.bus.read_cycle(access, clock).result.is_ok());
    }
    if phase > 1 {
        let clock = rig.bus.cpu_clock;
        rig.bus.idle(clock, RAM_TAS_MODIFY_CLOCKS);
    }
    if phase > 2 {
        let clock = rig.bus.cpu_clock;
        assert!(rig.bus.write_cycle(access, 0xa500, clock).result.is_ok());
    }
    assert!(!rig.bus.clock_fault);
    assert_eq!(
        rig.bus.rmw.as_ref().unwrap().phase,
        [
            Phase::Armed,
            Phase::ReadComplete,
            Phase::IdleComplete,
            Phase::WriteComplete
        ][usize::from(phase)]
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_whole_admission_is_read_only_with_trace_epochs_and_retained_ownership() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for cut in [0, 1, 61, 121] {
        for trace in [false, true] {
            for batched in [false, true] {
                let mut base = Rig::new(&[0; 32], cut, cut != 0, batched, 0, false);
                if trace {
                    base.bus.audio.trace = base
                        .bus
                        .audio
                        .trace
                        .prepare_with_clock(
                            32,
                            base.bus.rate(),
                            trace_chip(base.bus.region),
                            AudioTraceTiming::BusServiceBoundary,
                        )
                        .unwrap();
                }
                base.bus.audio.trace_origin = Some(main_origin(0x100, &base.bus.cartridge));
                base.bus.audio.write_psg(base.bus.now.get(), 0x90);
                assert_eq!(base.bus.audio.trace.is_enabled(), trace);
                assert_eq!(base.schedule.epoch.is_some(), cut != 0);
                if cut != 0 {
                    assert!(base.schedule.pending.is_none());
                    assert_eq!(base.sound.cycles(), 0);
                    let mut materialized = base.clone();
                    materialized.close_epoch();
                    assert_eq!(materialized.sound.cycles(), cut / 60 * 4);
                    assert!(materialized.schedule.pending.is_some());
                }
                let cpu = base.main();
                unchanged(&cpu, &base, QUALIFIED, Ok(cpu.clock()));
                record(&mut hash, (cut, trace, batched, "admitted", cpu.clock()));
                cases += 1;
                for kind in 0..15 {
                    let mut rig = base.clone();
                    if (4..8).contains(&kind) {
                        rmw(&mut rig, kind - 4);
                    }
                    let mut cpu = rig.main();
                    let mut qualification = QUALIFIED;
                    let expected = match kind {
                        0 => {
                            qualification = ResetQualification::Unqualified;
                            AdmissionError::Unqualified
                        }
                        1 => {
                            rig.bus.cpu_clock.0 += 1;
                            AdmissionError::Bus(BusError::ClockMismatch {
                                expected: rig.bus.cpu_clock,
                                actual: cpu.clock(),
                            })
                        }
                        2 => {
                            rig.bus.clock_fault = true;
                            AdmissionError::Bus(BusError::ClockOverflow)
                        }
                        3 => {
                            let fault = SoundFault::UnsupportedOpcode {
                                pc: 0x3210,
                                opcode: 0xff,
                            };
                            rig.schedule.fault = Some(fault);
                            AdmissionError::Sound(fault)
                        }
                        4..8 => AdmissionError::MainRmw,
                        8 => {
                            rig.schedule.rmw_checkpoint = Some(Box::new(
                                PendingSound::prepare(
                                    &rig.sound,
                                    &rig.bus.sound,
                                    &rig.bus.cartridge,
                                    rig.bus.now.get(),
                                )
                                .unwrap(),
                            ));
                            assert!(rig.bus.rmw.is_none());
                            AdmissionError::SoundRmwCheckpoint
                        }
                        9 => {
                            let clock = rig.bus.cpu_clock;
                            assert_eq!(
                                rig.bus.begin_reset_output(clock),
                                Ok(BusResponse::Complete(()))
                            );
                            assert!(rig.bus.reset_output);
                            AdmissionError::ResetOutput
                        }
                        10 | 11 => {
                            if kind == 10 {
                                std::sync::Arc::make_mut(&mut rig.bus.cartridge.bytes)[4..8]
                                    .copy_from_slice(&0x00400000u32.to_be_bytes());
                            }
                            let result =
                                reset(&mut cpu, &mut ResetEntry::new(rig.target(false)).unwrap());
                            assert_eq!(result.is_ok(), kind == 11);
                            AdmissionError::Cpu(if kind == 10 {
                                ExternalResetBoundaryError::HostFrozen
                            } else {
                                ExternalResetBoundaryError::NotOrdinaryBoundary
                            })
                        }
                        12 => {
                            cpu = PrefetchCpu::default();
                            assert_eq!(cpu.check_external_reset_boundary(), Ok(()));
                            AdmissionError::Cpu(ExternalResetBoundaryError::NotOrdinaryBoundary)
                        }
                        13 | 14 => {
                            cpu.registers.pc += if kind == 13 { 2 } else { 1 };
                            assert!(cpu.queued_words().is_some());
                            AdmissionError::Cpu(ExternalResetBoundaryError::NotOrdinaryBoundary)
                        }
                        _ => unreachable!(),
                    };
                    unchanged(&cpu, &rig, qualification, Err(expected.clone()));
                    record(
                        &mut hash,
                        (
                            cut,
                            trace,
                            batched,
                            kind,
                            expected,
                            cpu.clock(),
                            rig.bus.cpu_clock,
                            rig.bus.now,
                        ),
                    );
                    cases += 1;
                }
            }
        }
    }
    emit("md-reset-admission-v1", hash, cases);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_whole_admission_diagnostic_precedence_is_read_only() {
    let mut hash = 0xcbf29ce484222325;
    for kind in 0..6 {
        let mut rig = Rig::new(&[0; 32], 61, true, true, 0, false);
        let checkpoint = PendingSound::prepare(
            &rig.sound,
            &rig.bus.sound,
            &rig.bus.cartridge,
            rig.bus.now.get(),
        )
        .unwrap();
        if kind == 3 || kind == 4 {
            rmw(&mut rig, 0);
        }
        let mut cpu = rig.main();
        let mut qualification = QUALIFIED;
        let expected = match kind {
            0 => {
                qualification = ResetQualification::Unqualified;
                cpu = PrefetchCpu::default();
                AdmissionError::Unqualified
            }
            1 => {
                cpu = PrefetchCpu::default();
                rig.bus.clock_fault = true;
                AdmissionError::Cpu(ExternalResetBoundaryError::NotOrdinaryBoundary)
            }
            2 => {
                rig.bus.clock_fault = true;
                rig.bus.cpu_clock.0 += 1;
                AdmissionError::Bus(BusError::ClockOverflow)
            }
            3 => {
                let fault = SoundFault::ClockOverflow;
                rig.schedule.fault = Some(fault);
                AdmissionError::Sound(fault)
            }
            4 => {
                rig.schedule.rmw_checkpoint = Some(Box::new(checkpoint));
                AdmissionError::MainRmw
            }
            5 => {
                rig.schedule.rmw_checkpoint = Some(Box::new(checkpoint));
                let clock = rig.bus.cpu_clock;
                assert_eq!(
                    rig.bus.begin_reset_output(clock),
                    Ok(BusResponse::Complete(()))
                );
                AdmissionError::SoundRmwCheckpoint
            }
            _ => unreachable!(),
        };
        unchanged(&cpu, &rig, qualification, Err(expected.clone()));
        record(&mut hash, (kind, expected, cpu.clock(), rig.bus.cpu_clock));
    }
    emit("md-reset-admission-precedence-v1", hash, 6);
}
