use super::{cases::*, *};

fn byte(value: u8) -> DebugPortWrite {
    sound(0xc00011, value)
}

fn frozen(actual: &Emulator, before: &Emulator) {
    assert_eq!(actual.registers(), before.registers());
    assert_eq!(
        actual.cpu.queued_instruction_inputs(),
        before.cpu.queued_instruction_inputs()
    );
    assert_eq!(
        *actual.cpu.clone().interrupt_input(),
        *before.cpu.clone().interrupt_input()
    );
    assert_eq!(actual.cpu.trace_pending(), before.cpu.trace_pending());
    assert_eq!(actual.call_stack_status(), before.call_stack_status());
    assert_eq!(
        actual.call_stack().collect::<Vec<_>>(),
        before.call_stack().collect::<Vec<_>>()
    );
    assert_eq!(
        actual
            .cpu
            .clone()
            .drain_control_flow_events()
            .events()
            .count(),
        0
    );
}

pub(super) fn paired_ports(
    actual: &mut Emulator,
    expected: &mut Emulator,
    writes: &[DebugPortWrite],
) {
    let before = actual.clone();
    let report = actual.debug_write_ports(writes).unwrap();
    assert_eq!((report.instructions, report.writes), (0, writes.len()));
    assert_eq!(
        report.cpu_clocks.0,
        actual.cpu_clock().0 - before.cpu_clock().0
    );
    assert_eq!(
        Some(report.elapsed),
        actual.bus.now.checked_duration_since(before.bus.now)
    );
    assert!(report.cpu_clocks.0 >= writes.len() as u64 * 4);
    frozen(actual, &before);
    for write in writes {
        direct(expected, *write).unwrap();
    }
    assert_eq!(actual.cpu_clock(), expected.cpu_clock());
    assert_eq!(actual.bus.now, expected.bus.now);
    assert_eq!(actual.psg_snapshot(), expected.psg_snapshot());
    output(actual, expected);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn canonical_psg_latch_data_keeps_main_frozen_and_all_codecs_keep_future_audio() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            let mut actual = machine(region, timing, concurrent);
            let mut expected = machine(region, timing, concurrent);
            for emu in [&mut actual, &mut expected] {
                emu.set_call_stack_recording(true);
            }
            for (value, period, volume, latch) in [
                (0x85, 5, 15, 0),
                (0x12, 0x125, 15, 0),
                (0x90, 0x125, 0, 1),
                (0x07, 0x125, 7, 1),
            ] {
                paired_ports(&mut actual, &mut expected, &[byte(value), control(0x8f02)]);
                let psg = actual.psg_snapshot();
                assert_eq!(psg.periods, [period, 0, 0]);
                assert_eq!(psg.volumes, [volume, 15, 15, 15]);
                assert_eq!(psg.latched_register, latch);
            }
            paired_ports(&mut actual, &mut expected, &[byte(0x90)]);
            for format in 0..3 {
                let encode = |emu: &Emulator| match format {
                    0 => emu.encode_state().unwrap(),
                    1 => emu.encode_frontend_state().unwrap(),
                    _ => emu.encode_debugger_checkpoint().unwrap(),
                };
                let saved = encode(&actual);
                assert_eq!(saved, encode(&expected));
                let mut restored = machine(region, timing, concurrent);
                match format {
                    0 => restored.decode_state(&saved).unwrap(),
                    1 => restored.decode_frontend_state(&saved).unwrap(),
                    _ => restored.decode_debugger_checkpoint(&saved).unwrap(),
                }
                assert_eq!(encode(&restored), saved);
                if format == 2 {
                    assert_eq!(pcm(&restored), pcm(&actual));
                    assert_eq!(
                        restored.cpu.queued_instruction_inputs(),
                        actual.cpu.queued_instruction_inputs()
                    );
                }
                for _ in 0..2 {
                    restored.step_field().unwrap();
                    assert_eq!(restored.psg_snapshot().periods, [0x125, 0, 0]);
                    assert_eq!(restored.psg_snapshot().volumes, [0, 15, 15, 15]);
                    public_formats(&restored);
                    assert!(
                        pcm(&restored)
                            .iter()
                            .any(|bits| f32::from_bits(*bits) != 0.0)
                    );
                }
            }
            let saved = actual.encode_debugger_checkpoint().unwrap();
            let mut restored = machine(region, timing, concurrent);
            restored.decode_debugger_checkpoint(&saved).unwrap();
            for _ in 0..2 {
                actual.step_field().unwrap();
                expected.step_field().unwrap();
                restored.step_field().unwrap();
                output(&actual, &expected);
                output(&actual, &restored);
                assert_eq!(
                    actual.encode_debugger_checkpoint().unwrap(),
                    restored.encode_debugger_checkpoint().unwrap()
                );
                assert!(pcm(&actual).iter().any(|bits| f32::from_bits(*bits) != 0.0));
            }
        }
    }
    trace_and_hint_for(byte(0x90));
}

pub(super) fn running_sound(region: Region, timing: TimingModel) -> Emulator {
    let template = super::super::super::cpu_continuation_tests::machine(region, false);
    let mut rom = template.rom_bytes().to_vec();
    output::bootstrap_sound_rom(&mut rom, 0x210);
    Emulator::new_with_timing(
        rom,
        LoadConfig {
            region,
            concurrent_sound: true,
        },
        timing,
    )
    .unwrap()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn canonical_psg_at_actual_pending_sound_keeps_old_pcm_and_owned_continuation() {
    for region in REGIONS {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut actual = running_sound(region, timing);
            let mut expected = running_sound(region, timing);
            let deadline = actual.bus.now.get() + region.frame_ticks();
            let mut reached = false;
            for _ in 0..200_000 {
                if actual.bus.now.get() >= deadline {
                    break;
                }
                paired_instruction(&mut actual, &mut expected);
                reached = if timing == TimingModel::Precise {
                    let (pins, _) = actual.bus.timed_port_sound_phase().unwrap();
                    pins.mreq
                        && pins.read
                        && pins.address == Some(0x4000)
                        && actual.bus.owned_sound_cpu().unwrap().cycles() >= 400
                        && actual.sound_instruction_pending()
                } else {
                    actual.sound_cpu.cycles() >= 400
                        && actual
                            .sound_schedule
                            .pending
                            .as_ref()
                            .is_some_and(|pending| pending.deadline > actual.bus.now.get())
                };
                if reached {
                    break;
                }
            }
            assert!(
                reached,
                "real pending SOUND phase was not reached within one field"
            );
            output::assert_sound_program(&actual, region);
            output::assert_sound_program(&expected, region);
            let old_pcm = pcm(&actual);
            assert!(old_pcm.iter().any(|bits| f32::from_bits(*bits) != 0.0));
            paired_ports(
                &mut actual,
                &mut expected,
                &[byte(0x85), byte(0x12), byte(0x90)],
            );
            assert!(pcm(&actual).starts_with(&old_pcm));
            let saved = actual.encode_debugger_checkpoint().unwrap();
            let mut restored = running_sound(region, timing);
            restored.decode_debugger_checkpoint(&saved).unwrap();
            assert_eq!(restored.encode_debugger_checkpoint().unwrap(), saved);
            assert_eq!(pcm(&restored), pcm(&actual));
            let before = actual.bus.now;
            for _ in 0..128 {
                let step = instruction(&mut actual);
                assert_eq!(step, instruction(&mut expected));
                assert_eq!(step, instruction(&mut restored));
            }
            assert!(actual.bus.now > before);
            assert_eq!(actual.psg_snapshot().periods, [0x125, 0, 0]);
            assert_eq!(actual.psg_snapshot().volumes, [0, 15, 15, 15]);
            assert_eq!(
                actual.encode_debugger_checkpoint().unwrap(),
                restored.encode_debugger_checkpoint().unwrap()
            );
            output(&actual, &expected);
            output(&actual, &restored);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn psg_rejects_noncanonical_forms_and_discards_real_prefix_on_later_device_fault() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            let mut actual = machine(region, timing, concurrent);
            actual.set_call_stack_recording(true);
            if timing == TimingModel::Standard {
                actual.reset_and_begin_audio_trace(100).unwrap();
            }
            let before = actual.clone();
            for excluded in [
                sound(0xc00010, 0x90),
                sound(0xc00013, 0x90),
                sound(0xc00015, 0x90),
                sound(0x1c00011, 0x90),
                DebugPortWrite::Word {
                    address: 0xc00010,
                    value: 0x9090,
                },
                DebugPortWrite::Word {
                    address: 0xc00011,
                    value: 0x9090,
                },
            ] {
                assert_eq!(
                    actual.debug_write_ports(&[byte(0x90), excluded]),
                    Err(DebugPortError::UnsupportedPort { index: 1 })
                );
                same_checkpoint(&actual, &before);
                assert_eq!(actual.bus.audio, before.bus.audio);
            }
            let mut writes = vec![
                byte(0x90),
                control(0x8f02),
                control(if timing == TimingModel::Standard {
                    0x8c80
                } else {
                    0x8c02
                }),
            ];
            writes.extend([control(0x8f02); 64]);
            let mut prefix = before.clone();
            direct(&mut prefix, writes[0]).unwrap();
            direct(&mut prefix, writes[1]).unwrap();
            assert_eq!(prefix.psg_snapshot().volumes, [0, 15, 15, 15]);
            assert!(prefix.bus.now > before.bus.now);
            let mut failed = None;
            for (index, write) in writes.iter().copied().enumerate().skip(2) {
                if let Err(fault) = direct(&mut prefix, write) {
                    failed = Some((index, fault));
                    break;
                }
            }
            let (index, fault) = failed.expect("real VDP refusal was not reached");
            assert_eq!(
                actual.debug_write_ports(&writes),
                Err(DebugPortError::Execution { index, fault })
            );
            same_checkpoint(&actual, &before);
            assert_eq!(actual.bus.audio, before.bus.audio);
            actual.debug_write_ports(&[byte(0x90)]).unwrap();
            frozen(&actual, &before);
            if timing == TimingModel::Standard {
                assert_eq!(
                    actual.finish_audio_trace().unwrap().trace.invalidated,
                    Some(AudioTraceInvalidation::ExternalMutation)
                );
                assert_eq!(
                    before
                        .clone()
                        .finish_audio_trace()
                        .unwrap()
                        .trace
                        .invalidated,
                    None
                );
            }
        }
    }
}
