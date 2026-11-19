use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::timing::TimedOutcome;

fn origin(rig: &mut Rig, concurrent: bool, kind: u8) -> PrefetchCpu {
    let mut registers = rig.main().registers;
    let traced = matches!(kind, 2 | 3 | 5);
    let stopped = kind >= 4;
    registers.set_status_register(
        0x15 | if kind & 1 != 0 || stopped { 0x2000 } else { 0 } | if traced { 0x8000 } else { 0 },
    );
    let words: [u16; 2] = if stopped {
        [0x4e72, if traced { 0xa715 } else { 0x2715 }]
    } else {
        [0x4e71; 2]
    };
    for (bytes, word) in std::sync::Arc::make_mut(&mut rig.bus.cartridge.bytes)[0x100..0x104]
        .as_chunks_mut::<2>()
        .0
        .iter_mut()
        .zip(words)
    {
        *bytes = word.to_be_bytes();
    }
    let mut cpu = PrefetchCpu::from_prefetched(registers, words, rig.bus.cpu_clock).unwrap();
    if kind >= 2 {
        let step = if concurrent {
            cpu.step_instruction(&mut rig.scheduled()).unwrap()
        } else {
            cpu.step_instruction(&mut rig.bus).unwrap()
        };
        assert_eq!(step.opcode, words[0]);
        assert_eq!(step.exception, None);
    }
    assert_eq!(cpu.stopped(), stopped);
    assert_eq!(cpu.trace_pending(), traced);
    assert_eq!(cpu.clock(), rig.bus.cpu_clock);
    assert_eq!(cpu.check_external_reset_ready_boundary(), Ok(()));
    cpu
}

fn bytes(cpu: &PrefetchCpu) -> Vec<u8> {
    let mut writer = StateWriter::new();
    cpu.write_boundary_state(&mut writer).unwrap();
    writer.into_bytes()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_scope_real_stop_trace_absent_epoch_and_routed_level7() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for enabled in [false, true] {
                for epoch in [false, true] {
                    for level in [0, 7] {
                        for kind in 0..6 {
                            let mut actual = fixture(region, &[0; 32], 61, epoch, 48_000);
                            std::sync::Arc::make_mut(&mut actual.bus.cartridge.bytes)[0x7c..0x80]
                                .copy_from_slice(&0x00ff0240u32.to_be_bytes());
                            actual.bus.ram[0x240..0x248]
                                .copy_from_slice(&[0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71]);
                            let mut cpu = origin(&mut actual, concurrent, kind);
                            actual.bus.irq_level = level;
                            cpu.interrupt_input().set_level(level);
                            assert_eq!(actual.schedule.epoch.is_some(), epoch);
                            trace(&mut actual, enabled);
                            let original = cpu.clone();
                            let mut control = actual.clone();
                            let before = Terminal::capture(&cpu, &actual.bus, &actual.schedule);
                            let report = entered(run(
                                &mut cpu,
                                &mut actual.bus,
                                (&mut actual.sound, &mut actual.schedule),
                                concurrent,
                                QUALIFIED,
                                &mut crate::sound_trace::NoSoundCapture,
                            ));
                            assert_eq!(report.origin, before);
                            assert_eq!(report.completion, Ok(()));
                            assert_eq!(report.finalization, Some(Ok(())));
                            assert_eq!(report.elapsed_cpu, Some(40));
                            assert_eq!(report.elapsed_bus, Some(40));
                            assert_eq!(report.elapsed_master, Some(280));
                            assert!(report.adapter.is_none() && report.idle.is_none());
                            assert_eq!(cpu.clock().0, original.clock().0 + 40);
                            assert!(!cpu.stopped() && !cpu.trace_pending());
                            assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
                            assert_eq!(cpu.check_external_reset_ready_boundary(), Ok(()));
                            assert_eq!(cpu.registers.d, original.registers.d);
                            assert_eq!(cpu.registers.a, original.registers.a);
                            assert_eq!(cpu.registers.usp, original.registers.usp);
                            assert_eq!(cpu.registers.ssp, 0xff8000);
                            assert_eq!(cpu.registers.pc, 0xff0200);
                            assert_eq!(cpu.registers.status_register(), 0x2715);
                            assert_eq!(cpu.queued_words(), Some([0x5279, 0x00ff]));
                            callback_control(&mut control, concurrent);
                            assert_eq!(actual, control);
                            assert_eq!(format!("{actual:?}"), format!("{control:?}"));
                            for rig in [&mut actual, &mut control] {
                                clean(rig, enabled, AudioTraceInvalidation::ExternalMutation);
                            }
                            let saved = bytes(&cpu);
                            let mut restored = PrefetchCpu::default();
                            let mut reader = StateReader::new(&saved);
                            restored.read_boundary_state(&mut reader).unwrap();
                            assert!(reader.is_exhausted());
                            assert_eq!(restored, cpu);
                            assert_eq!(bytes(&restored), saved);
                            for edge in [false, true] {
                                let mut running = cpu.clone();
                                let mut decoded = restored.clone();
                                let mut live = actual.clone();
                                let mut copy = control.clone();
                                for (cpu, rig) in
                                    [(&mut running, &mut live), (&mut decoded, &mut copy)]
                                {
                                    if edge {
                                        rig.bus.irq_level = 7;
                                        cpu.interrupt_input().set_level(0);
                                        cpu.interrupt_input().set_level(7);
                                    } else {
                                        cpu.interrupt_input().set_level(level);
                                    }
                                }
                                let outcome = if concurrent {
                                    running.step_boundary(&mut live.scheduled()).unwrap()
                                } else {
                                    running.step_boundary(&mut live.bus).unwrap()
                                };
                                let other = if concurrent {
                                    decoded.step_boundary(&mut copy.scheduled()).unwrap()
                                } else {
                                    decoded.step_boundary(&mut copy.bus).unwrap()
                                };
                                assert_eq!(outcome, other);
                                if edge {
                                    assert!(matches!(
                                        outcome,
                                        TimedOutcome::Interrupt {
                                            level: 7,
                                            vector: 31,
                                            ..
                                        }
                                    ));
                                    assert_eq!(
                                        &live.bus.ram[0x300..0x302],
                                        &0x1234u16.to_be_bytes()
                                    );
                                } else {
                                    assert!(matches!(outcome, TimedOutcome::Instruction(_)));
                                    assert_eq!(
                                        &live.bus.ram[0x300..0x302],
                                        &0x1235u16.to_be_bytes()
                                    );
                                }
                                assert_eq!(running, decoded);
                                live.advance(concurrent, 50_000);
                                copy.advance(concurrent, 50_000);
                                assert_eq!(live, copy);
                                let pcm = live.pcm();
                                assert_eq!(pcm, copy.pcm());
                                assert!(pcm.iter().any(|v| v & 0x7fffffff != 0));
                                record(&mut hash, (edge, outcome, running, pcm));
                            }
                            record(
                                &mut hash,
                                (
                                    region, concurrent, enabled, epoch, level, kind, report, saved,
                                ),
                            );
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    emit("md-reset-scope-origins-v1", hash, cases);
}
