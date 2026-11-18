use super::super::super::admission::AdmissionError;
use super::*;
use std::sync::Arc;
use zeff_m68k::timing::physical::PhysicalInspection;
use zeff_m68k::timing::reset_boundary::ExternalResetBoundaryError;

fn retry(
    cpu: &mut PrefetchCpu,
    rig: &mut Rig,
    concurrent: bool,
    boundary: ExternalResetBoundaryError,
) {
    let before = (cpu.clone(), rig.clone());
    assert_eq!(
        run(
            cpu,
            &mut rig.bus,
            (&mut rig.sound, &mut rig.schedule),
            concurrent,
            QUALIFIED,
            &mut crate::sound_trace::NoSoundCapture,
        ),
        Outcome::Rejected(Rejection::Admission(AdmissionError::Cpu(boundary)))
    );
    assert_eq!((&*cpu, &*rig), (&before.0, &before.1));
    assert_eq!(
        format!("{cpu:?}, {rig:?}"),
        format!("{:?}, {:?}", before.0, before.1)
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_scope_failures_keep_latches_partial_clocks_and_close_owned_scopes() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for enabled in [false, true] {
                for kind in 0..4 {
                    let mut actual = fixture(region, &[0; 32], 61, true, 48_000);
                    trace(&mut actual, enabled);
                    let mut cpu = actual.main();
                    let registers = cpu.registers;
                    if kind == 0 || kind == 3 {
                        actual.bus.now = MasterTicks::new(u64::MAX - 1);
                    }
                    if kind == 3 {
                        let clock = CpuClocks(u64::MAX - u64::from(u32::MAX));
                        cpu = PrefetchCpu::from_prefetched(
                            cpu.registers,
                            cpu.queued_words().unwrap(),
                            clock,
                        )
                        .unwrap();
                        actual.bus.cpu_clock = clock;
                    }
                    if kind == 1 || kind == 2 {
                        let target = if kind == 1 { 0x400000u32 } else { 0x101u32 };
                        Arc::make_mut(&mut actual.bus.cartridge.bytes)[4..8]
                            .copy_from_slice(&target.to_be_bytes());
                    }
                    let origin = Terminal::capture(&cpu, &actual.bus, &actual.schedule);
                    let report = entered(run(
                        &mut cpu,
                        &mut actual.bus,
                        (&mut actual.sound, &mut actual.schedule),
                        concurrent,
                        QUALIFIED,
                        &mut crate::sound_trace::NoSoundCapture,
                    ));
                    assert_eq!(report.origin, origin);
                    assert_eq!(
                        report.terminal,
                        Terminal::capture(&cpu, &actual.bus, &actual.schedule)
                    );
                    assert_eq!(report.completion, Err(CompletionError::Primitive));
                    assert_eq!(report.finalization, None);
                    assert_eq!(cpu.registers, registers);
                    assert_eq!(cpu.queued_words(), None);
                    assert!(!cpu.stopped());
                    if kind == 0 || kind == 3 {
                        let source = ResetBusError::Bus(BusError::ClockOverflow);
                        assert_eq!(report.adapter, Some(source.clone()));
                        assert_eq!(
                            report.idle,
                            Some(IdleFailure {
                                start: origin.cpu_clock,
                                requested: 14,
                                completed: 0,
                                source: source.clone()
                            })
                        );
                        assert_eq!(report.elapsed_cpu, Some(14));
                        assert_eq!(report.elapsed_bus, Some(0));
                        assert_eq!(report.elapsed_master, Some(0));
                        assert_eq!(report.terminal.master.get(), u64::MAX - 1);
                        assert_eq!(report.terminal.vdp.get(), 61);
                        assert!(report.terminal.clock_fault);
                        if kind == 3 {
                            assert_eq!(report.entry, Some(Err(EntryFault::ClockOverflow)));
                        } else {
                            assert!(
                                matches!(&report.entry,Some(Err(EntryFault::HostFailure{source:actual,..})) if *actual==source)
                            );
                        }
                    } else {
                        let elapsed = if kind == 1 { 34 } else { 30 };
                        assert_eq!(report.elapsed_cpu, Some(elapsed));
                        assert_eq!(report.elapsed_bus, Some(elapsed));
                        assert_eq!(report.elapsed_master, Some(elapsed * MAIN_CPU_DIVISOR));
                        assert_eq!(report.terminal.master, report.terminal.vdp);
                        assert!(!report.terminal.clock_fault);
                        if kind == 1 {
                            let cause = ResetBusError::Bus(BusError::UnsupportedAccess {
                                address: 0x400000,
                                direction: Direction::Read,
                            });
                            assert_eq!(report.adapter, Some(cause.clone()));
                            assert!(
                                matches!(&report.entry,Some(Err(EntryFault::HostFailure{source,..})) if *source==cause)
                            );
                        } else {
                            assert!(matches!(
                                &report.entry,
                                Some(Err(EntryFault::TimingUnavailable(_)))
                            ));
                            assert_eq!(report.adapter, None);
                            assert_eq!(report.terminal.inspection, PhysicalInspection::Unavailable);
                            assert!(matches!(
                                report.terminal.physical,
                                PhysicalEntryState::DoubleFaultHalted { .. }
                            ));
                        }
                        assert_eq!(report.idle, None);
                    }
                    clean(
                        &mut actual,
                        enabled,
                        AudioTraceInvalidation::ExternalMutation,
                    );
                    retry(
                        &mut cpu,
                        &mut actual,
                        concurrent,
                        if kind == 2 {
                            ExternalResetBoundaryError::NotOrdinaryBoundary
                        } else {
                            ExternalResetBoundaryError::HostFrozen
                        },
                    );
                    record(
                        &mut hash,
                        (
                            region,
                            concurrent,
                            enabled,
                            kind,
                            report,
                            cpu.clock(),
                            actual.sound.cycles(),
                        ),
                    );
                    cases += 1;
                }
            }
        }
    }
    emit("md-reset-scope-failures-v1", hash, cases);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_scope_sound_faults_at_i14_read_and_i2_preserve_first_cause() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for enabled in [false, true] {
            for (nops, cut, idle, elapsed) in [
                (0, 60, Some(14), 14),
                (0, 40, None, 18),
                (4, 144, Some(2), 36),
            ] {
                let mut program = vec![0; nops];
                program.extend([0x3a, 0, 0x7f]);
                let mut actual = fixture(region, &program, cut, false, 48_000);
                trace(&mut actual, enabled);
                let mut cpu = actual.main();
                let registers = cpu.registers;
                let report = entered(run(
                    &mut cpu,
                    &mut actual.bus,
                    (&mut actual.sound, &mut actual.schedule),
                    true,
                    QUALIFIED,
                    &mut crate::sound_trace::NoSoundCapture,
                ));
                let fault = SoundFault::UnsupportedAccess {
                    address: 0x7f00,
                    direction: Direction::Read,
                };
                let cause = ResetBusError::Sound(fault);
                assert_eq!(report.completion, Err(CompletionError::Primitive));
                assert_eq!(report.finalization, None);
                assert_eq!(report.adapter, Some(cause.clone()));
                assert_eq!(report.terminal.sound_fault, Some(fault));
                assert!(
                    matches!(&report.entry,Some(Err(EntryFault::HostFailure{source,..})) if *source==cause)
                );
                assert_eq!(report.elapsed_cpu, Some(elapsed));
                assert_eq!(report.elapsed_bus, Some(elapsed));
                assert_eq!(report.elapsed_master, Some(elapsed * MAIN_CPU_DIVISOR));
                assert_eq!(
                    report.terminal,
                    Terminal::capture(&cpu, &actual.bus, &actual.schedule)
                );
                assert_eq!(cpu.registers, registers);
                assert_eq!(report.terminal.clock_fault, idle.is_some());
                match idle {
                    Some(clocks) => {
                        let failure = report.idle.as_ref().unwrap();
                        assert_eq!(
                            (failure.requested, failure.completed),
                            (clocks, u64::from(clocks))
                        );
                        assert_eq!(failure.source, cause);
                    }
                    None => assert_eq!(report.idle, None),
                }
                clean(
                    &mut actual,
                    enabled,
                    AudioTraceInvalidation::ExternalMutation,
                );
                retry(
                    &mut cpu,
                    &mut actual,
                    true,
                    ExternalResetBoundaryError::HostFrozen,
                );
                record(&mut hash, (region, enabled, nops, cut, report));
                cases += 1;
            }
        }
    }
    emit("md-reset-scope-sound-faults-v1", hash, cases);
}
