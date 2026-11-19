use super::{cases::*, psg::paired_ports, *};

const PORTS: [(u32, u32, u8); 2] = [(0xa1000f, 0xa10013, 0x53), (0xa10015, 0xa10019, 0x4d)];

fn ready(region: Region, timing: TimingModel, concurrent: bool) -> Emulator {
    let template = machine(region, timing, concurrent);
    let mut rom = template.rom_bytes().to_vec();
    let mut program = vec![0x4e71; 1000];
    program.extend([0x7003, 0x60fc, 0x4e71]);
    words(&mut rom, 0x200, &program);
    let mut emu = Emulator::new_with_timing(
        rom,
        LoadConfig {
            region,
            concurrent_sound: concurrent,
        },
        timing,
    )
    .unwrap();
    for _ in 0..1000 {
        instruction(&mut emu);
    }
    assert_eq!(emu.registers().pc, 0x9d0);
    assert_eq!(emu.queued_words().unwrap()[0], 0x7003);
    assert!(emu.bus.now.get() >= 24_206);
    emu
}

fn transmit(actual: &mut Emulator, expected: &mut Emulator, region: Region, timing: TimingModel) {
    for (port, (tx, control_address, value)) in PORTS.into_iter().enumerate() {
        paired_ports(actual, expected, &[sound(control_address, 0x38)]);
        let mut input = actual.bus.input.clone();
        let begin = actual.bus.now.get() + u64::from(timing == TimingModel::Precise);
        let clock = actual.cpu_clock();
        paired_ports(actual, expected, &[sound(tx, value)]);
        let end = actual.bus.now.get() + u64::from(timing == TimingModel::Precise);
        assert_eq!(actual.cpu_clock().0 - clock.0, 4);
        assert_eq!(end - begin, 28);
        assert!(input.write_serial_at_range(port * 3, value, begin, end, region));
        assert_eq!(actual.bus.input, input);
        assert_eq!(actual.bus.input, expected.bus.input);
        assert_eq!(actual.bus.input.read_serial(port * 3, end, region), value);
        assert_eq!(actual.peek(control_address), None);
        assert!(!actual.bus.input.serial_read_allowed(port * 3 + 2));
        assert!(!actual.bus.input.data_read_allowed(port));
        assert!(matches!(
            actual
                .bus
                .input
                .read_serial_at_range(port * 3 + 2, end, end + 28, region),
            Some(0x38 | 0x39)
        ));
        let input = actual.bus.input.clone();
        paired_ports(actual, expected, &[sound(control_address, 0x38)]);
        assert_eq!(actual.bus.input, input);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn canonical_uart_transfers_keep_main_frozen_and_all_codecs_keep_pending_loads() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            let mut actual = ready(region, timing, concurrent);
            let mut expected = ready(region, timing, concurrent);
            let patch = CheatPatch::WideRomWrite {
                address: 0x9d1,
                value: CheatValue::constant(9),
            };
            for emu in [&mut actual, &mut expected] {
                emu.install_rom_patches(&[patch]).unwrap();
                emu.set_call_stack_recording(true);
                assert_eq!(emu.peek(0x9d1), Some(9));
                assert_eq!(emu.queued_words().unwrap()[0], 0x7003);
            }
            transmit(&mut actual, &mut expected, region, timing);
            paired_ports(
                &mut actual,
                &mut expected,
                &[
                    sound(0xc00011, 0x85),
                    sound(0xc00011, 0x12),
                    sound(0xc00011, 0x90),
                ],
            );
            assert_eq!(
                actual.cpu.queued_instruction_inputs(),
                expected.cpu.queued_instruction_inputs()
            );
            let end = actual.bus.input_state_tick().unwrap();
            for port in 0..2 {
                assert_eq!(
                    actual
                        .bus
                        .input
                        .read_serial_at_range(port * 3 + 2, end - 28, end, region),
                    Some(0x39)
                );
            }
            for format in 0..3 {
                let encode = |emu: &Emulator| match format {
                    0 => emu.encode_state().unwrap(),
                    1 => emu.encode_frontend_state().unwrap(),
                    _ => emu.encode_debugger_checkpoint().unwrap(),
                };
                let saved = encode(&actual);
                assert_eq!(saved, encode(&expected));
                let mut restored = ready(region, timing, concurrent);
                restored.install_rom_patches(&[patch]).unwrap();
                match format {
                    0 => restored.decode_state(&saved).unwrap(),
                    1 => restored.decode_frontend_state(&saved).unwrap(),
                    _ => restored.decode_debugger_checkpoint(&saved).unwrap(),
                }
                assert_eq!(encode(&restored), saved);
                assert_eq!(restored.bus.input, actual.bus.input);
                assert_eq!(restored.peek(0x9d1), Some(9));
                assert_eq!(restored.queued_words().unwrap()[0], 0x7003);
                if format == 2 {
                    assert_eq!(
                        restored.cpu.queued_instruction_inputs(),
                        actual.cpu.queued_instruction_inputs()
                    );
                    assert_eq!(pcm(&restored), pcm(&actual));
                }
                let before = restored.bus.now;
                instruction(&mut restored);
                assert_eq!(restored.registers().d[0], 3);
                for _ in 0..2 {
                    restored.step_field().unwrap();
                }
                assert!(restored.bus.now.get() > before.get() + 12 * 11_200);
                assert_eq!(restored.registers().d[0], 9);
                assert!(
                    pcm(&restored)
                        .iter()
                        .any(|bits| f32::from_bits(*bits) != 0.0)
                );
                let end = restored.bus.now.get();
                for port in 0..2 {
                    assert_eq!(
                        restored.bus.input.read_serial_at_range(
                            port * 3 + 2,
                            end - 28,
                            end,
                            region
                        ),
                        Some(0x38)
                    );
                }
            }
            let saved = actual.encode_debugger_checkpoint().unwrap();
            let mut restored = ready(region, timing, concurrent);
            restored.install_rom_patches(&[patch]).unwrap();
            restored.decode_debugger_checkpoint(&saved).unwrap();
            for _ in 0..2 {
                actual.step_field().unwrap();
                expected.step_field().unwrap();
                restored.step_field().unwrap();
                output(&actual, &expected);
                output(&actual, &restored);
                assert_eq!(actual.bus.input, restored.bus.input);
                assert_eq!(
                    actual.encode_debugger_checkpoint().unwrap(),
                    restored.encode_debugger_checkpoint().unwrap()
                );
                assert!(pcm(&actual).iter().any(|bits| f32::from_bits(*bits) != 0.0));
            }
        }
    }
    trace_and_hint_for(sound(0xa10013, 0));
}

fn traced_ready(region: Region, concurrent: bool) -> Emulator {
    let template = machine(region, TimingModel::Standard, concurrent);
    let mut rom = template.rom_bytes().to_vec();
    let mut program = vec![0x4e71; 1000];
    for (tx, control_address, value) in PORTS {
        for (address, value) in [(control_address, 0x38), (tx, value)] {
            program.extend([
                0x13fc,
                u16::from(value),
                (address >> 16) as u16,
                address as u16,
            ]);
        }
    }
    program.extend([0x7003, 0x60fc, 0x4e71]);
    words(&mut rom, 0x200, &program);
    let mut emu = Emulator::new_with_timing(
        rom,
        LoadConfig {
            region,
            concurrent_sound: concurrent,
        },
        TimingModel::Standard,
    )
    .unwrap();
    emu.reset_and_begin_audio_trace(100).unwrap();
    for _ in 0..1004 {
        instruction(&mut emu);
    }
    assert_eq!(emu.registers().pc, 0x9f0);
    assert!(emu.bus.now.get() >= 24_206);
    for (port, (_, _, value)) in PORTS.into_iter().enumerate() {
        assert!(!emu.bus.input.serial_read_allowed(port * 3 + 2));
        assert_eq!(
            emu.bus
                .input
                .read_serial(port * 3, emu.bus.now.get(), region),
            value
        );
    }
    emu
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn uart_modes_and_noncanonical_forms_reject_atomically_after_real_prefix() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            let mut actual = ready(region, timing, concurrent);
            let mut expected = ready(region, timing, concurrent);
            transmit(&mut actual, &mut expected, region, timing);
            if timing == TimingModel::Standard {
                actual = traced_ready(region, concurrent);
            }
            let before = actual.clone();
            actual.debug_write_ports(&[]).unwrap();
            same_checkpoint(&actual, &before);
            for bad in [
                sound(0xa1000e, 1),
                sound(0xa10011, 1),
                sound(0xa10012, 1),
                sound(0xa10014, 1),
                sound(0xa10017, 1),
                sound(0xa10018, 1),
                sound(0xa1001b, 1),
                sound(0xa1001f, 1),
                sound(0xa1002f, 1),
                sound(0x1a1000f, 1),
                DebugPortWrite::Word {
                    address: 0xa1000e,
                    value: 0x5353,
                },
                DebugPortWrite::Word {
                    address: 0xa10012,
                    value: 0x3838,
                },
                DebugPortWrite::Word {
                    address: 0xa10014,
                    value: 0x4d4d,
                },
                DebugPortWrite::Word {
                    address: 0xa10018,
                    value: 0x3838,
                },
                DebugPortWrite::Word {
                    address: 0xa1000f,
                    value: 0x5353,
                },
            ] {
                assert_eq!(
                    actual.debug_write_ports(&[sound(0xc00011, 0x90), bad]),
                    Err(DebugPortError::UnsupportedPort { index: 1 })
                );
                same_checkpoint(&actual, &before);
            }
            for (_, control_address, _) in PORTS {
                let writes = [sound(0xc00011, 0x90), sound(control_address, 0x20)];
                let mut prefix = before.clone();
                direct(&mut prefix, writes[0]).unwrap();
                assert!(prefix.bus.now > before.bus.now);
                let fault = direct(&mut prefix, writes[1]).unwrap_err();
                assert_eq!(
                    prefix.psg_snapshot().volumes[0],
                    0,
                    "{region:?}/{timing:?}/{concurrent}/{control_address:06x}"
                );
                assert_eq!(
                    actual.debug_write_ports(&writes),
                    Err(DebugPortError::Execution { index: 1, fault })
                );
                same_checkpoint(&actual, &before);
                assert_eq!(actual.bus.audio, before.bus.audio);
                assert_eq!(actual.input_fault(), None);
            }
            actual.debug_write_ports(&[sound(0xa10013, 0x38)]).unwrap();
            assert_eq!(actual.bus.input, before.bus.input);
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
