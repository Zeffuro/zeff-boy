use super::super::scope::{CompletionError, Outcome, Rejection, Report, Terminal, run};
use super::*;
use zeff_emu_common::audio_trace::AudioTraceInvalidation;
use zeff_m68k::timing::physical::PhysicalEntryState;

mod faults;
mod origins;
mod rig;
use rig::*;
const QUALIFIED: ResetQualification = ResetQualification::QualifiedSafeAssertionAndRelease;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_scope_success_matches_ordinary_callbacks_epochs_and_future_pcm() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for enabled in [false, true] {
                for cut in [1, 61, 121] {
                    for rate in [44_100, 48_000] {
                        let mut actual = fixture(region, &[0; 32], cut, true, rate);
                        trace(&mut actual, enabled);
                        let mut control = actual.clone();
                        let mut cpu = actual.main();
                        let original = cpu.clone();
                        let origin = Terminal::capture(&cpu, &actual.bus, &actual.schedule);
                        let report = entered(run(
                            &mut cpu,
                            &mut actual.bus,
                            (&mut actual.sound, &mut actual.schedule),
                            concurrent,
                            QUALIFIED,
                            &mut crate::sound_trace::NoSoundCapture,
                        ));
                        assert_eq!(report.completion, Ok(()));
                        assert_eq!(report.finalization, Some(Ok(())));
                        assert_eq!(report.origin, origin);
                        assert_eq!(
                            report.terminal,
                            Terminal::capture(&cpu, &actual.bus, &actual.schedule)
                        );
                        assert_eq!(report.elapsed_cpu, Some(40));
                        assert_eq!(report.elapsed_bus, Some(40));
                        assert_eq!(report.elapsed_master, Some(280));
                        assert_eq!(report.entry.as_ref().unwrap().as_ref().unwrap().clocks, 40);
                        assert!(report.adapter.is_none() && report.idle.is_none());
                        assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
                        assert_eq!(cpu.check_external_reset_ready_boundary(), Ok(()));
                        callback_control(&mut control, concurrent);
                        assert_eq!(actual, control);
                        assert_eq!(format!("{actual:?}"), format!("{control:?}"));
                        let mut registers = original.registers;
                        registers.pc = 0xff0200;
                        registers.ssp = 0xff8000;
                        registers.set_status_register((registers.status_register() & 31) | 0x2700);
                        let mut expected = PrefetchCpu::from_prefetched(
                            registers,
                            [0x5279, 0x00ff],
                            CpuClocks(original.clock().0 + 40),
                        )
                        .unwrap();
                        expected
                            .interrupt_input()
                            .set_level(control.bus.interrupt_level());
                        assert_eq!(cpu, expected);
                        for rig in [&mut actual, &mut control] {
                            clean(rig, enabled, AudioTraceInvalidation::ExternalMutation);
                        }
                        if concurrent {
                            cpu.step_boundary(&mut actual.scheduled()).unwrap();
                            expected.step_boundary(&mut control.scheduled()).unwrap();
                        } else {
                            cpu.step_boundary(&mut actual.bus).unwrap();
                            expected.step_boundary(&mut control.bus).unwrap();
                        }
                        assert_eq!(cpu, expected);
                        assert_eq!(&actual.bus.ram[0x300..0x302], &0x1235u16.to_be_bytes());
                        actual.advance(concurrent, 50_000);
                        control.advance(concurrent, 50_000);
                        assert_eq!(actual, control);
                        let pcm = actual.pcm();
                        assert_eq!(pcm, control.pcm());
                        assert!(pcm.iter().any(|v| v & 0x7fffffff != 0));
                        assert_eq!(actual.bus.audio.sample_rate(), rate);
                        record(
                            &mut hash,
                            (region, concurrent, enabled, cut, rate, report, cpu, pcm),
                        );
                        cases += 1;
                    }
                }
            }
        }
    }
    emit("md-reset-scope-success-v1", hash, cases);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_scope_rejections_precede_mutation_and_prior_trace_reason_survives() {
    use super::super::admission::AdmissionError;
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for enabled in [false, true] {
                for kind in 0..4 {
                    let mut actual = fixture(region, &[0; 32], 61, true, 48_000);
                    trace(&mut actual, enabled);
                    let mut qualification = QUALIFIED;
                    let reason = match kind {
                        0 => {
                            actual.bus.audio.begin_batch();
                            Rejection::AudioBatchActive
                        }
                        1 => {
                            qualification = ResetQualification::Unqualified;
                            actual.bus.audio.begin_batch();
                            Rejection::Admission(AdmissionError::Unqualified)
                        }
                        2 => {
                            actual.schedule.fault = Some(SoundFault::ClockOverflow);
                            Rejection::Admission(AdmissionError::Sound(SoundFault::ClockOverflow))
                        }
                        3 => {
                            actual.schedule.rmw_checkpoint = Some(Box::new(
                                PendingSound::prepare(
                                    &actual.sound,
                                    &actual.bus.sound,
                                    &actual.bus.cartridge,
                                    actual.bus.now.get(),
                                )
                                .unwrap(),
                            ));
                            Rejection::Admission(AdmissionError::SoundRmwCheckpoint)
                        }
                        _ => unreachable!(),
                    };
                    let mut cpu = actual.main();
                    let before = (cpu.clone(), actual.clone());
                    let debug = format!("{before:?}");
                    for _ in 0..2 {
                        assert_eq!(
                            run(
                                &mut cpu,
                                &mut actual.bus,
                                (&mut actual.sound, &mut actual.schedule),
                                concurrent,
                                qualification,
                                &mut crate::sound_trace::NoSoundCapture,
                            ),
                            Outcome::Rejected(reason.clone())
                        );
                        assert_eq!((&cpu, &actual), (&before.0, &before.1));
                        assert_eq!(format!("{:?}", (cpu.clone(), actual.clone())), debug);
                    }
                    record(&mut hash, (region, concurrent, enabled, kind, reason));
                    cases += 1;
                }
            }
            let mut actual = fixture(region, &[0; 32], 61, true, 48_000);
            trace(&mut actual, true);
            actual
                .bus
                .audio
                .trace
                .invalidate(AudioTraceInvalidation::Reset);
            let mut cpu = actual.main();
            let report = entered(run(
                &mut cpu,
                &mut actual.bus,
                (&mut actual.sound, &mut actual.schedule),
                concurrent,
                QUALIFIED,
                &mut crate::sound_trace::NoSoundCapture,
            ));
            assert_eq!(report.completion, Ok(()));
            clean(&mut actual, true, AudioTraceInvalidation::Reset);
            record(&mut hash, (region, concurrent, report));
            cases += 1;
        }
    }
    emit("md-reset-scope-rejections-v1", hash, cases);
}
