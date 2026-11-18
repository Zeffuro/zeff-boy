use super::{cases::*, psg::paired_ports, *};

fn ready(region: Region, timing: TimingModel, concurrent: bool) -> Result<Emulator, LoadError> {
    let template = machine(region, timing, concurrent);
    let mut rom = template.rom_bytes().to_vec();
    rom.resize(0x100000, 0);
    rom[0x1b0..0x1bc].copy_from_slice(b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\x3f\xff");
    Emulator::new_with_timing(
        rom,
        LoadConfig {
            region,
            concurrent_sound: concurrent,
        },
        timing,
    )
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn ordinary_header_sram_uses_real_lanes_protection_and_all_codec_continuations() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            if timing == TimingModel::Precise {
                assert!(matches!(ready(region, timing, concurrent),
                    Err(LoadError::TimingProfile(reason))
                        if reason == "cartridge hardware is not qualified for owned timing"));
                continue;
            }
            let mut actual = ready(region, timing, concurrent).unwrap();
            let mut expected = ready(region, timing, concurrent).unwrap();
            for emu in [&mut actual, &mut expected] {
                emu.set_call_stack_recording(true);
                emu.install_rom_patches(&[CheatPatch::WideRomWrite {
                    address: 0x201,
                    value: CheatValue::constant(9),
                }])
                .unwrap();
                assert_eq!(emu.queued_words().unwrap()[0], 0x7003);
                assert_eq!(emu.peek(0x201), Some(9));
                assert_eq!(
                    emu.debug_save_ports(),
                    Some(DebugSavePorts {
                        data_start: 0x200000,
                        data_end: 0x203fff,
                        control: 0xa130f1,
                        enabled: true,
                        write_protected: false,
                    })
                );
            }
            for (address, value, first, last, protected) in [
                (0x200001, 0xa5, 0xa5, 0xff, false),
                (0x200000, 0x5a, 0xa5, 0xff, false),
                (0x203fff, 0x3c, 0xa5, 0x3c, false),
                (0xa130f1, 3, 0xa5, 0x3c, true),
                (0x200001, 0x55, 0xa5, 0x3c, true),
                (0xa130f1, 1, 0xa5, 0x3c, false),
                (0x200001, 0x5a, 0x5a, 0x3c, false),
            ] {
                paired_ports(&mut actual, &mut expected, &[sound(address, value)]);
                assert_eq!(actual.peek(0x200000), Some(0xff));
                assert_eq!(actual.peek(0x200001), Some(first));
                assert_eq!(actual.peek(0x203fff), Some(last));
                let ports = actual.debug_save_ports().unwrap();
                assert!(ports.enabled);
                assert_eq!(ports.write_protected, protected);
                assert_eq!(actual.dump_save_ram(), expected.dump_save_ram());
                let cells = actual.dump_save_ram().unwrap();
                assert_eq!((cells.len(), cells[0], cells[8191]), (8192, first, last));
                assert!(cells[1..8191].iter().all(|&value| value == 0xff));
            }
            paired_ports(
                &mut actual,
                &mut expected,
                &[
                    sound(0xc00011, 0x85),
                    sound(0xc00011, 0x12),
                    sound(0xc00011, 0x90),
                ],
            );
            for format in 0..3 {
                let encode = |emu: &Emulator| match format {
                    0 => emu.encode_state().unwrap(),
                    1 => emu.encode_frontend_state().unwrap(),
                    _ => emu.encode_debugger_checkpoint().unwrap(),
                };
                let saved = encode(&actual);
                assert_eq!(saved, encode(&expected));
                let mut restored = ready(region, timing, concurrent).unwrap();
                restored
                    .install_rom_patches(&[CheatPatch::WideRomWrite {
                        address: 0x201,
                        value: CheatValue::constant(9),
                    }])
                    .unwrap();
                match format {
                    0 => restored.decode_state(&saved).unwrap(),
                    1 => restored.decode_frontend_state(&saved).unwrap(),
                    _ => restored.decode_debugger_checkpoint(&saved).unwrap(),
                }
                assert_eq!(encode(&restored), saved);
                assert_eq!(restored.debug_save_ports(), actual.debug_save_ports());
                assert_eq!(restored.dump_save_ram(), actual.dump_save_ram());
                if format == 2 {
                    assert_eq!(pcm(&restored), pcm(&actual));
                    assert_eq!(
                        restored.cpu.queued_instruction_inputs(),
                        actual.cpu.queued_instruction_inputs()
                    );
                }
                instruction(&mut restored);
                assert_eq!(restored.registers().d[0], 3);
                for _ in 0..2 {
                    restored.step_field().unwrap();
                    assert_eq!(restored.dump_save_ram(), actual.dump_save_ram());
                    assert!(
                        pcm(&restored)
                            .iter()
                            .any(|bits| f32::from_bits(*bits) != 0.0)
                    );
                }
                assert_eq!(restored.registers().d[0], 9);
            }
            let saved = actual.encode_debugger_checkpoint().unwrap();
            let mut restored = ready(region, timing, concurrent).unwrap();
            restored
                .install_rom_patches(&[CheatPatch::WideRomWrite {
                    address: 0x201,
                    value: CheatValue::constant(9),
                }])
                .unwrap();
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
                assert_eq!(actual.dump_save_ram(), expected.dump_save_ram());
                assert!(pcm(&actual).iter().any(|bits| f32::from_bits(*bits) != 0.0));
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn ordinary_sram_faulting_control_discards_real_prefix_and_unadmitted_holes() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            if timing == TimingModel::Precise {
                assert!(matches!(ready(region, timing, concurrent),
                    Err(LoadError::TimingProfile(reason))
                        if reason == "cartridge hardware is not qualified for owned timing"));
                continue;
            }
            let mut actual = ready(region, timing, concurrent).unwrap();
            actual.set_call_stack_recording(true);
            actual.cpu.registers.set_status_register(0x2000 | TRACE);
            assert!(actual.cpu.interrupt_input().set_level(6));
            assert!(actual.set_interrupt_level(7));
            let before = actual.clone();
            actual.debug_write_ports(&[sound(0x200001, 0xa5)]).unwrap();
            assert_eq!(actual.registers(), before.registers());
            assert_eq!(
                actual.cpu.queued_instruction_inputs(),
                before.cpu.queued_instruction_inputs()
            );
            assert_eq!(
                *actual.cpu.clone().interrupt_input(),
                *before.cpu.clone().interrupt_input()
            );
            assert_eq!(actual.interrupt_level(), 7);
            assert!(!actual.cpu.trace_pending());
            let before = actual.clone();
            for bad in [
                sound(0x1fffff, 1),
                sound(0x204000, 1),
                sound(0x1200001, 1),
                sound(0xa130f0, 1),
                sound(0xa130f3, 1),
                sound(0xa130ff, 1),
                DebugPortWrite::Word {
                    address: 0x200000,
                    value: 0xa55a,
                },
                DebugPortWrite::Word {
                    address: 0xa130f0,
                    value: 1,
                },
            ] {
                assert_eq!(
                    actual.debug_write_ports(&[sound(0x200003, 0x44), bad]),
                    Err(DebugPortError::UnsupportedPort { index: 1 })
                );
                same_checkpoint(&actual, &before);
            }
            for value in [0, 2, 4, 0xff] {
                let writes = [sound(0x200003, 0x44), sound(0xa130f1, value)];
                let mut prefix = before.clone();
                direct(&mut prefix, writes[0]).unwrap();
                assert_eq!(prefix.peek(0x200003), Some(0x44));
                assert!(prefix.bus.now > before.bus.now);
                let fault = direct(&mut prefix, writes[1]).unwrap_err();
                assert_eq!(
                    actual.debug_write_ports(&writes),
                    Err(DebugPortError::Execution { index: 1, fault })
                );
                same_checkpoint(&actual, &before);
                assert_eq!(actual.bus.audio, before.bus.audio);
            }
            let mut hardware = before.clone();
            let mut write = Write(DebugDataWrite::Byte {
                address: 0x204001,
                value: 0x5a,
            });
            if hardware.bus.owned.is_some() {
                hardware.step_owned_operation(&mut write).unwrap();
            } else {
                hardware
                    .step_main_operation_with(&mut write, &mut crate::sound_trace::NoSoundCapture)
                    .unwrap();
            }
            assert_eq!(hardware.dump_save_ram(), before.dump_save_ram());
            assert!(hardware.bus.now > before.bus.now);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn timed_sram_requires_factory_seal_exact_header_geometry_and_exclusive_owner() {
    use crate::cartridge::{Cartridge, ram, registers::LockOn};
    for (timing, concurrent) in PROFILES {
        if timing == TimingModel::Precise {
            assert!(matches!(ready(Region::NtscUs, timing, concurrent),
                Err(LoadError::TimingProfile(reason))
                    if reason == "cartridge hardware is not qualified for owned timing"));
            continue;
        }
        let eligible = ready(Region::NtscUs, timing, concurrent).unwrap();
        for variant in 0..8 {
            let mut emu = eligible.clone();
            match variant {
                0 => {
                    *std::sync::Arc::make_mut(&mut emu.bus.cartridge.bytes)
                        .last_mut()
                        .unwrap() ^= 1;
                }
                1 => emu.bus.cartridge.save = Some(ram::headerless_odd8_fixture()),
                2 => {
                    emu.bus.cartridge.save.as_mut().unwrap().bytes =
                        vec![0xff; 512].into_boxed_slice()
                }
                3 => emu.bus.cartridge.lock_on = Some(LockOn::TwoMiB),
                4 => emu.bus.cartridge.save = None,
                5 => {
                    let mut rom = eligible.rom_bytes().to_vec();
                    rom[0x1b0..0x1bc].fill(0);
                    emu.bus.cartridge = Cartridge::new(rom).unwrap();
                    emu.bus.cartridge.save = eligible.bus.cartridge.save.clone();
                }
                6 => {
                    let mut rom = eligible.rom_bytes().to_vec();
                    rom[0x1b9] = 0x20;
                    rom[0x1ba] = 0xff;
                    emu.bus.cartridge = Cartridge::new(rom).unwrap();
                }
                _ => emu.bus.cartridge.save.as_mut().unwrap().enabled = false,
            }
            let before = emu.clone();
            assert_eq!(emu.debug_save_ports(), None, "variant {variant}");
            assert_eq!(
                emu.debug_write_ports(&[sound(0xc00011, 0x90), sound(0x200001, 0xa5)]),
                Err(DebugPortError::UnsupportedPort { index: 1 })
            );
            assert_eq!(emu, before);
            assert_eq!(emu.bus.now, before.bus.now);
        }
        for profile in ram::profile_tests::overlay_fixture::ALL {
            let mut emu = eligible.clone();
            emu.bus.cartridge =
                ram::profile_tests::overlay_fixture::cartridge(profile, &[0x60fe, 0x4e71]);
            let before = emu.clone();
            assert_eq!(emu.debug_save_ports(), None, "{profile:?}");
            assert_eq!(
                emu.debug_write_ports(&[sound(0x200001, 0xa5)]),
                Err(DebugPortError::UnsupportedPort { index: 0 })
            );
            assert_eq!(emu, before);
        }
        for (size, product, declaration) in [
            (
                0x300000,
                b"GM G-5524  -00",
                b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\x3f\xff",
            ),
            (
                0x10000,
                b"GM T-12046 -00",
                b"RA\xe8\x40\x00\x20\x00\x01\x00\x20\x00\x01",
            ),
        ] {
            let mut rom = eligible.rom_bytes().to_vec();
            rom.resize(size, 0);
            rom[0x180..0x18e].copy_from_slice(product);
            rom[0x1b0..0x1bc].copy_from_slice(declaration);
            let mut emu = eligible.clone();
            emu.bus.cartridge = Cartridge::new(rom).unwrap();
            let before = emu.clone();
            assert_eq!(emu.debug_save_ports(), None);
            assert_eq!(
                emu.debug_write_ports(&[sound(0xa130f1, 1)]),
                Err(DebugPortError::UnsupportedPort { index: 0 })
            );
            assert_eq!(emu, before);
        }
    }
}
