use super::{cases::*, psg::paired_ports, *};

fn granted(emu: &Emulator) -> bool {
    let memory = emu.bus.active_sound_memory();
    memory.granted
        && !memory.reset_asserted
        && emu
            .bus
            .timed_port_sound_phase()
            .is_none_or(|(pins, _)| pins.bus_ack)
}

fn wait_grant(actual: &mut Emulator, expected: &mut Emulator) {
    for _ in 0..256 {
        assert_eq!(granted(actual), granted(expected));
        if granted(actual) {
            return;
        }
        paired_instruction(actual, expected);
    }
    panic!("real SOUND request did not reach an observed grant");
}

fn held_guest(
    region: Region,
    timing: TimingModel,
    concurrent: bool,
    trace: bool,
) -> (Emulator, Emulator) {
    let program = [
        0x13fc, 1, 0xa1, 0x1100, 0x13fc, 1, 0xa1, 0x1200, 0x4e71, 0x60fc,
    ];
    let mut actual = guest(region, timing, concurrent, &program);
    let mut expected = guest(region, timing, concurrent, &program);
    if trace {
        actual.reset_and_begin_audio_trace(100).unwrap();
        expected.reset_and_begin_audio_trace(100).unwrap();
    }
    wait_grant(&mut actual, &mut expected);
    (actual, expected)
}

fn dac_writes() -> Vec<DebugPortWrite> {
    [
        (0xa04002, 0xb6),
        (0xa04003, 0xc0),
        (0xa04000, 0x2a),
        (0xa04001, 0xff),
        (0xa04000, 0x2b),
        (0xa04001, 0x80),
    ]
    .into_iter()
    .map(|(address, value)| sound(address, value))
    .collect()
}

fn codec_future(
    actual: &mut Emulator,
    expected: &mut Emulator,
    region: Region,
    timing: TimingModel,
    concurrent: bool,
) {
    for _ in 0..4 {
        if actual.registers().pc == 0x200 {
            break;
        }
        paired_instruction(actual, expected);
    }
    assert_eq!(actual.registers().pc, 0x200);
    assert_eq!(actual.queued_words().unwrap()[0], 0x7003);
    let patch = CheatPatch::WideRomWrite {
        address: 0x201,
        value: CheatValue::constant(9),
    };
    for emu in [&mut *actual, &mut *expected] {
        emu.install_rom_patches(&[patch]).unwrap();
        assert_eq!(emu.peek(0x201), Some(9));
        assert_eq!(emu.queued_words().unwrap()[0], 0x7003);
    }
    paired_ports(actual, expected, &[sound(0xa04000, 0x2a), control(0x8f02)]);
    paired_ports(actual, expected, &[sound(0xa04001, 0xe0)]);
    for format in 0..3 {
        let encode = |emu: &Emulator| match format {
            0 => emu.encode_state().unwrap(),
            1 => emu.encode_frontend_state().unwrap(),
            _ => emu.encode_debugger_checkpoint().unwrap(),
        };
        let saved = encode(actual);
        assert_eq!(saved, encode(expected));
        let mut left = machine(region, timing, concurrent);
        {
            let restored = &mut left;
            restored.install_rom_patches(&[patch]).unwrap();
            match format {
                0 => restored.decode_state(&saved).unwrap(),
                1 => restored.decode_frontend_state(&saved).unwrap(),
                _ => restored.decode_debugger_checkpoint(&saved).unwrap(),
            }
            assert_eq!(encode(restored), saved);
            assert_eq!(restored.peek(0x201), Some(9));
            assert_eq!(restored.queued_words().unwrap()[0], 0x7003);
            assert!(granted(restored));
            assert_eq!(restored.bus.active_sound_memory().ram[0x1fff], 0x24);
            assert_eq!(restored.bus.active_sound_memory().bank, 0x155);
        }
        if format == 2 {
            assert_eq!(pcm(&left), pcm(actual));
            assert_eq!(
                left.cpu.queued_instruction_inputs(),
                actual.cpu.queued_instruction_inputs()
            );
        }
        instruction(&mut left);
        assert_eq!(left.registers().d[0], 3);
        assert_eq!(left.opn2_snapshot().dac, 0xe0);
        assert!(left.opn2_snapshot().dac_enabled);
        for _ in 0..2 {
            left.step_field().unwrap();
            assert_eq!(left.registers().d[0], 9);
            assert!(pcm(&left).iter().any(|bits| f32::from_bits(*bits) != 0.0));
        }
    }
    let saved = actual.encode_debugger_checkpoint().unwrap();
    let mut restored = machine(region, timing, concurrent);
    restored.install_rom_patches(&[patch]).unwrap();
    restored.decode_debugger_checkpoint(&saved).unwrap();
    for _ in 0..2 {
        actual.step_field().unwrap();
        expected.step_field().unwrap();
        restored.step_field().unwrap();
        output(actual, expected);
        output(actual, &restored);
        assert_eq!(
            actual.encode_debugger_checkpoint().unwrap(),
            restored.encode_debugger_checkpoint().unwrap()
        );
        assert!(pcm(actual).iter().any(|bits| f32::from_bits(*bits) != 0.0));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn timed_sound_aperture_obeys_closed_reset_and_granted_ram_ym_bank_with_all_codecs() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            let mut actual = machine(region, timing, concurrent);
            let mut expected = machine(region, timing, concurrent);
            for reset in [true, false] {
                assert_eq!(actual.bus.active_sound_memory().reset_asserted, reset);
                assert!(!actual.bus.active_sound_memory().requested && !granted(&actual));
                paired_ports(
                    &mut actual,
                    &mut expected,
                    &[
                        sound(0xa00010, 0xa5),
                        sound(0xa04000, 0x2a),
                        sound(0xa04001, 0xff),
                        sound(0xa06000, 1),
                        control(0x8f02),
                    ],
                );
                assert_eq!(actual.bus.active_sound_memory().ram[0x10], 0);
                assert_eq!(actual.bus.active_sound_memory().bank, 0);
                assert_eq!(actual.opn2_snapshot().address, 0);
                assert_eq!(actual.opn2_snapshot().dac, 0x80);
                assert!(!actual.opn2_snapshot().dac_enabled);
                if reset {
                    paired_ports(
                        &mut actual,
                        &mut expected,
                        &[sound(0xa11200, 1), control(0x8f02)],
                    );
                }
            }
            paired_ports(
                &mut actual,
                &mut expected,
                &[sound(0xa11100, 1), control(0x8f02)],
            );
            wait_grant(&mut actual, &mut expected);
            for emu in [&mut actual, &mut expected] {
                emu.set_call_stack_recording(true);
            }
            let mut writes = vec![
                sound(0xa00000, 0x5a),
                sound(0xa00001, 0xa5),
                sound(0xa01ffe, 0x42),
                sound(0xa01fff, 0x24),
            ];
            writes.extend(dac_writes());
            writes.push(control(0x8f02));
            paired_ports(&mut actual, &mut expected, &writes);
            for (address, value) in [(0, 0x5a), (1, 0xa5), (0x1ffe, 0x42), (0x1fff, 0x24)] {
                assert_eq!(actual.bus.active_sound_memory().ram[address], value);
            }
            for (bit, bank) in [
                (1, 0x100),
                (0, 0x80),
                (1, 0x140),
                (0, 0xa0),
                (1, 0x150),
                (0, 0xa8),
                (1, 0x154),
                (0, 0xaa),
                (1, 0x155),
            ] {
                paired_ports(
                    &mut actual,
                    &mut expected,
                    &[sound(0xa06000, bit), control(0x8f02)],
                );
                assert_eq!(actual.bus.active_sound_memory().bank, bank);
            }
            assert_eq!(actual.opn2_snapshot().address, 0x2b);
            assert_eq!(actual.opn2_snapshot().dac, 0xff);
            assert!(actual.opn2_snapshot().dac_enabled);
            codec_future(&mut actual, &mut expected, region, timing, concurrent);
        }
    }
    trace_and_hint_for(sound(0xa00010, 0xa5));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn timed_aperture_request_from_real_pending_sound_waits_for_observed_owner_then_resumes() {
    for region in REGIONS {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut actual = psg::running_sound(region, timing);
            let mut expected = psg::running_sound(region, timing);
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
                            .is_some_and(|pending| pending.deadline > actual.bus.now.get() + 56)
                };
                if reached {
                    break;
                }
            }
            assert!(reached, "no real pending SOUND phase within one field");
            output::assert_sound_program(&actual, region);
            output::assert_sound_program(&expected, region);
            assert!(!actual.bus.active_sound_memory().requested && !granted(&actual));
            if timing == TimingModel::Standard {
                let mut waiting = actual.clone();
                let mut waiting_control = expected.clone();
                let clock = waiting.cpu_clock();
                let deadline = waiting.sound_schedule.pending.as_ref().unwrap().deadline;
                paired_ports(
                    &mut waiting,
                    &mut waiting_control,
                    &[sound(0xa11100, 1), sound(0xa01f00, 0x5a)],
                );
                assert!(waiting.cpu_clock().0 - clock.0 > 8);
                assert!(waiting.bus.now.get() >= deadline);
                assert!(granted(&waiting));
                assert_eq!(waiting.bus.active_sound_memory().ram[0x1f00], 0x5a);
            }
            let old_pcm = pcm(&actual);
            let dac = actual.opn2_snapshot().dac;
            assert!(old_pcm.iter().any(|bits| f32::from_bits(*bits) != 0.0));
            paired_ports(
                &mut actual,
                &mut expected,
                &[
                    sound(0xa01f00, 0xa5),
                    sound(0xa06000, 1),
                    sound(0xa04000, 0x2a),
                    sound(0xa04001, 0),
                    control(0x8f02),
                ],
            );
            assert_eq!(actual.bus.active_sound_memory().ram[0x1f00], 0);
            assert_eq!(actual.bus.active_sound_memory().bank, 0);
            assert_eq!(actual.opn2_snapshot().dac, dac);
            paired_ports(&mut actual, &mut expected, &[sound(0xa11100, 1)]);
            wait_grant(&mut actual, &mut expected);
            paired_ports(
                &mut actual,
                &mut expected,
                &[
                    sound(0xa01f00, 0xa5),
                    sound(0xa06000, 1),
                    sound(0xa04000, 0x2a),
                    sound(0xa04001, 0xe0),
                    control(0x8f02),
                ],
            );
            assert_eq!(actual.bus.active_sound_memory().ram[0x1f00], 0xa5);
            assert_eq!(actual.bus.active_sound_memory().bank, 0x100);
            assert_eq!(actual.opn2_snapshot().dac, 0xe0);
            assert!(pcm(&actual).starts_with(&old_pcm));
            let saved = actual.encode_debugger_checkpoint().unwrap();
            let mut restored = psg::running_sound(region, timing);
            restored.decode_debugger_checkpoint(&saved).unwrap();
            assert_eq!(restored.encode_debugger_checkpoint().unwrap(), saved);
            let counter = actual.bus.active_sound_memory().ram[0x700];
            for emu in [&mut actual, &mut expected, &mut restored] {
                emu.debug_write_ports(&[sound(0xa11100, 0), control(0x8f02)])
                    .unwrap();
            }
            let mut resumed = false;
            for _ in 0..20_000 {
                let step = instruction(&mut actual);
                assert_eq!(step, instruction(&mut expected));
                assert_eq!(step, instruction(&mut restored));
                if actual.bus.active_sound_memory().ram[0x700] != counter {
                    resumed = true;
                    break;
                }
            }
            assert!(
                resumed,
                "SOUND guest counter did not resume after actual release"
            );
            assert!(!granted(&actual));
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
fn aperture_rejects_mirrors_and_words_and_rolls_back_ram_ym_bank_after_real_fault() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            let (mut actual, _) =
                held_guest(region, timing, concurrent, timing == TimingModel::Standard);
            let before = actual.clone();
            for excluded in [
                sound(0xa02000, 1),
                sound(0xa04004, 1),
                sound(0xa06001, 1),
                sound(0xa07f11, 1),
                sound(0x1a00000, 1),
                DebugPortWrite::Word {
                    address: 0xa00000,
                    value: 0x5aa5,
                },
                DebugPortWrite::Word {
                    address: 0xa04000,
                    value: 0x2aff,
                },
                DebugPortWrite::Word {
                    address: 0xa06000,
                    value: 1,
                },
            ] {
                assert_eq!(
                    actual.debug_write_ports(&[sound(0xa01f00, 0xa5), excluded]),
                    Err(DebugPortError::UnsupportedPort { index: 1 })
                );
                same_checkpoint(&actual, &before);
                assert_eq!(actual.bus.audio, before.bus.audio);
            }
            let mut writes = vec![
                sound(0xa01f00, 0xa5),
                sound(0xa06000, 1),
                sound(0xa04000, 0x2a),
                sound(0xa04001, 0xe0),
                control(0x8f02),
                control(if timing == TimingModel::Standard {
                    0x8c80
                } else {
                    0x8c02
                }),
            ];
            writes.extend([control(0x8f02); 64]);
            let mut prefix = before.clone();
            for write in &writes[..5] {
                direct(&mut prefix, *write).unwrap();
            }
            assert_eq!(prefix.bus.active_sound_memory().ram[0x1f00], 0xa5);
            assert_eq!(prefix.bus.active_sound_memory().bank, 0x100);
            assert_eq!(prefix.opn2_snapshot().dac, 0xe0);
            assert!(prefix.bus.now > before.bus.now);
            let mut failed = None;
            for (index, write) in writes.iter().copied().enumerate().skip(5) {
                if let Err(fault) = direct(&mut prefix, write) {
                    failed = Some((index, fault));
                    break;
                }
            }
            let (index, fault) = failed.expect("actual later VDP refusal was not reached");
            assert_eq!(
                actual.debug_write_ports(&writes),
                Err(DebugPortError::Execution { index, fault })
            );
            same_checkpoint(&actual, &before);
            assert_eq!(actual.bus.audio, before.bus.audio);
            actual.debug_write_ports(&[sound(0xa01f00, 0xa5)]).unwrap();
            if timing == TimingModel::Standard {
                assert_eq!(
                    before
                        .clone()
                        .finish_audio_trace()
                        .unwrap()
                        .trace
                        .invalidated,
                    None
                );
                assert_eq!(
                    actual.finish_audio_trace().unwrap().trace.invalidated,
                    Some(AudioTraceInvalidation::ExternalMutation)
                );
            }
        }
    }
}
