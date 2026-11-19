use super::fixture::{Rom, config, expected, run_to_phase};
use super::layouts::{header_profile, mapped};
use zeff_md_core::{Emulator, Region};

fn fixture(size: usize, region: Region, concurrent: bool) -> Emulator {
    fixture_at(size, region, concurrent, 0x200000, None)
}

fn fixture_at(
    size: usize,
    region: Region,
    concurrent: bool,
    base: u32,
    profile: Option<usize>,
) -> Emulator {
    let mut rom = Rom::new(size);
    rom.enable();
    rom.psg();
    rom.byte(base + 1, 0x47);
    rom.byte(base + size as u32 * 2 - 1, 0xb3);
    let bytes = mapped(rom.finish(), base);
    let bytes = if let Some(index) = profile {
        header_profile(bytes, index)
    } else {
        bytes
    };
    let mut machine = Emulator::new(bytes, config(region, concurrent)).unwrap();
    machine.load_save_ram(&expected(size, 0x93)).unwrap();
    machine.reset_and_begin_audio_trace(8).unwrap();
    run_to_phase(&mut machine, 3);
    machine
}

fn unchanged(machine: &Emulator, before: &Emulator) {
    assert_eq!(machine, before);
    let mut actual = machine.clone();
    let mut expected = before.clone();
    assert_eq!(actual.finish_audio_trace(), expected.finish_audio_trace());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn save_and_state_rejections_preserve_complete_machine_and_active_trace() {
    for size in [512, 8192, 32768] {
        for concurrent in [false, true] {
            let mut machine = fixture(size, Region::Pal, concurrent);
            let before = machine.clone();
            let saved = machine.encode_state().unwrap();
            assert_eq!(&saved[8..12], &1_u32.to_le_bytes());
            let save = crate::state_layout::save_range(&saved);
            let section = save.start - 4;
            let presence = save.start;
            assert_eq!(save.len(), size + 3);
            let enabled = presence + 1;
            assert_eq!(
                &saved[section..presence],
                &((size + 3) as u32).to_le_bytes()
            );
            assert_eq!(&saved[presence..enabled + 1], &[1, 1]);
            for length in [
                0,
                size - 1,
                size + 1,
                if size == 8192 { 32768 } else { 8192 },
            ] {
                assert!(machine.load_save_ram(&vec![0; length]).is_err());
                unchanged(&machine, &before);
            }
            let mut malformed = Vec::new();
            for cut in [0, 12, section, presence, enabled, saved.len() - 1] {
                malformed.push(saved[..cut].to_vec());
            }
            for (offset, value) in [(presence, 0), (presence, 2), (enabled, 2)] {
                let mut bytes = saved.clone();
                bytes[offset] = value;
                malformed.push(bytes);
            }
            if size != 512 {
                let mut disabled = saved.clone();
                disabled[enabled] = 0;
                malformed.push(disabled);
            }
            for length in [size + 2, size + 4] {
                let mut bytes = saved.clone();
                bytes[section..presence].copy_from_slice(&(length as u32).to_le_bytes());
                malformed.push(bytes);
            }
            let mut trailing = saved.clone();
            trailing.push(0);
            malformed.push(trailing);
            let mut extra_section = saved.clone();
            extra_section.insert(save.end, 0);
            extra_section[section..presence].copy_from_slice(&((size + 4) as u32).to_le_bytes());
            malformed.push(extra_section);
            let mut invalid_protection = saved.clone();
            invalid_protection[save.end - 1] = 2;
            malformed.push(invalid_protection);
            let other_size = if size == 8192 { 32768 } else { 8192 };
            let other = fixture(other_size, Region::Pal, concurrent);
            let foreign = other.encode_state().unwrap();
            malformed.push(foreign.clone());
            let mut forged_identity = foreign;
            forged_identity[12..44].copy_from_slice(&saved[12..44]);
            malformed.push(forged_identity);
            for bytes in malformed {
                assert!(
                    machine.decode_state(&bytes).is_err(),
                    "size={size} bytes={}",
                    bytes.len()
                );
                unchanged(&machine, &before);
            }
            assert!(
                machine
                    .clone()
                    .finish_audio_trace()
                    .unwrap()
                    .trace
                    .events
                    .len()
                    >= 3
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn nontrivial_state_roundtrip_restores_cells_frame_and_future_active_audio() {
    for (size, base, profile) in [
        (512, 0x200000, None),
        (8192, 0x200000, None),
        (32768, 0x200000, None),
        (8192, 0x300000, None),
        (32768, 0x300000, None),
        (8192, 0x200000, Some(0)),
        (8192, 0x200000, Some(1)),
        (8192, 0x200000, Some(2)),
    ] {
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for concurrent in [false, true] {
                for frontend in [false, true] {
                    let mut machine = fixture_at(size, region, concurrent, base, profile);
                    let front = machine.encode_frontend_state().unwrap();
                    let saved = machine.encode_state().unwrap();
                    let cells = machine.dump_save_ram();
                    let mut restored = machine.clone();
                    restored.load_save_ram(&vec![0x11; size]).unwrap();
                    if frontend {
                        restored.decode_frontend_state(&front).unwrap();
                        assert_eq!(restored.encode_frontend_state().unwrap(), front);
                    } else {
                        restored.decode_state(&saved).unwrap();
                        machine.decode_state(&saved).unwrap();
                    }
                    assert_eq!(restored.dump_save_ram(), cells);
                    assert_eq!(restored.encode_state().unwrap(), saved);
                    machine.drain_audio_samples();
                    restored.drain_audio_samples();
                    for _ in 0..3000 {
                        let outcome = machine.step_instruction();
                        assert_eq!(outcome, restored.step_instruction());
                    }
                    assert_eq!(
                        machine.encode_state().unwrap(),
                        restored.encode_state().unwrap()
                    );
                    assert_eq!(machine.framebuffer(), restored.framebuffer());
                    let audio = machine.drain_audio_samples();
                    assert!(!audio.is_empty());
                    assert!(audio.iter().any(|sample| *sample != 0.0));
                    assert_eq!(audio, restored.drain_audio_samples());
                    restored.load_save_ram(&vec![0x55; size]).unwrap();
                    assert_ne!(restored.dump_save_ram(), machine.dump_save_ram());
                    assert_eq!(restored.rom_bytes(), machine.rom_bytes());
                }
            }
        }
    }
    protected_continuations();
    upper_control_continuations();
}

fn upper_control_continuations() {
    use zeff_m68k::timing::TimingFault;
    use zeff_md_core::{BusError, MachineFault};
    for size in [8192, 32768] {
        for protected in [false, true] {
            let mut rom = Rom::new(size);
            rom.psg();
            if protected {
                rom.byte(0xa130f1, 3);
            }
            rom.phase(1);
            rom.byte(0xa130f0, 1);
            rom.phase(2);
            for (offset, cell) in [0, size / 2, size - 1].into_iter().enumerate() {
                rom.read_byte(0x200001 + cell as u32 * 2, 0xff0000 + offset as u32);
            }
            rom.enable();
            rom.byte(0x200001, 0x42);
            let bytes = rom.finish();
            for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
                for concurrent in [false, true] {
                    for frontend in [false, true] {
                        let mut machine =
                            Emulator::new(bytes.clone(), config(region, concurrent)).unwrap();
                        let cells = expected(size, 0x93);
                        machine.load_save_ram(&cells).unwrap();
                        machine.reset_and_begin_audio_trace(8).unwrap();
                        run_to_phase(&mut machine, 1);
                        let saved = machine.encode_state().unwrap();
                        let front = machine.encode_frontend_state().unwrap();
                        let original_rom = machine.rom_bytes().to_vec();
                        let mut restored = machine.clone();
                        if frontend {
                            restored.decode_frontend_state(&front).unwrap();
                            machine.decode_frontend_state(&front).unwrap();
                        } else {
                            restored.decode_state(&saved).unwrap();
                            machine.decode_state(&saved).unwrap();
                        }
                        machine.drain_audio_samples();
                        restored.drain_audio_samples();
                        let outcome = machine.step_instruction();
                        assert_eq!(outcome, restored.step_instruction());
                        if protected {
                            assert!(matches!(
                                outcome,
                                Err(MachineFault::Main(TimingFault::Bus {
                                    source: BusError::UnsupportedAccess {
                                        address: 0xa130f0,
                                        direction: zeff_m68k::bus::Direction::Write,
                                    },
                                    ..
                                }))
                            ));
                            let mut reset_cpu = machine.clone();
                            reset_cpu.reset_cpu().unwrap();
                            assert_eq!(
                                &reset_cpu.encode_state().unwrap()[8..12],
                                &1_u32.to_le_bytes()
                            );
                        } else {
                            outcome.unwrap();
                        }
                        assert_eq!(machine.dump_save_ram(), Some(cells.clone()));
                        assert_eq!(restored.dump_save_ram(), Some(cells.clone()));
                        assert_eq!(machine.rom_bytes(), original_rom);
                        assert_eq!(machine, restored);
                        if !protected {
                            run_to_phase(&mut machine, 3);
                            run_to_phase(&mut restored, 3);
                            for (offset, cell) in [0, size / 2, size - 1].into_iter().enumerate() {
                                assert_eq!(
                                    machine.peek(0xff0000 + offset as u32),
                                    Some(cells[cell])
                                );
                            }
                            let mut changed = cells;
                            changed[0] = 0x42;
                            assert_eq!(machine.dump_save_ram(), Some(changed));
                            for _ in 0..3000 {
                                machine.step_instruction().unwrap();
                                restored.step_instruction().unwrap();
                            }
                        }
                        assert_eq!(machine, restored);
                        assert_eq!(machine.framebuffer(), restored.framebuffer());
                        let audio = machine.drain_audio_samples();
                        if !protected {
                            assert!(audio.iter().any(|sample| *sample != 0.0));
                        }
                        assert_eq!(audio, restored.drain_audio_samples());
                        assert_eq!(machine.finish_audio_trace(), restored.finish_audio_trace());
                    }
                }
            }
        }
    }
}

fn protected_continuations() {
    use zeff_emu_common::time::Reset;
    for (size, base) in [
        (8192, 0x200000),
        (32768, 0x200000),
        (8192, 0x300000),
        (32768, 0x300000),
    ] {
        let mut rom = Rom::new(size);
        rom.psg();
        rom.phase(1);
        rom.byte(0xa130f1, 3);
        rom.phase(2);
        rom.read_byte(base + 1, 0xff0000);
        rom.byte(base + 1, 0x99);
        rom.write_word(base + 2, 0x115a);
        rom.write_long(base + 4, 0x22663377);
        rom.phase(4);
        rom.enable();
        rom.write_long(base, 0x11223344);
        rom.phase(5);
        let bytes = mapped(rom.finish(), base);
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for concurrent in [false, true] {
                for frontend in [false, true] {
                    let mut machine =
                        Emulator::new(bytes.clone(), config(region, concurrent)).unwrap();
                    machine.load_save_ram(&vec![0x47; size]).unwrap();
                    run_to_phase(&mut machine, 1);
                    let old = machine.encode_state().unwrap();
                    assert_eq!(&old[8..12], &1_u32.to_le_bytes());
                    run_to_phase(&mut machine, 2);
                    let saved = machine.encode_state().unwrap();
                    assert_eq!(&saved[8..12], &1_u32.to_le_bytes());
                    let front = machine.encode_frontend_state().unwrap();
                    let mut restored = machine.clone();
                    restored.load_save_ram(&vec![0x55; size]).unwrap();
                    assert_eq!(
                        &restored.encode_state().unwrap()[8..12],
                        &1_u32.to_le_bytes()
                    );
                    let mut imported = restored.clone();
                    run_to_phase(&mut imported, 4);
                    assert_eq!(imported.dump_save_ram(), Some(vec![0x55; size]));
                    assert_eq!(imported.peek(0xff0000), Some(0x55));
                    if frontend {
                        restored.decode_frontend_state(&front).unwrap();
                        assert_eq!(restored.encode_frontend_state().unwrap(), front);
                    } else {
                        restored.decode_state(&saved).unwrap();
                        machine.decode_state(&saved).unwrap();
                    }
                    assert_eq!(restored.encode_state().unwrap(), saved);
                    let before = restored.clone();
                    let mut malformed = saved.clone();
                    let flag = crate::state_layout::save_range(&malformed).end - 1;
                    malformed[flag] = 2;
                    assert!(restored.decode_state(&malformed).is_err());
                    unchanged(&restored, &before);
                    let mut legacy = restored.clone();
                    legacy.decode_state(&old).unwrap();
                    assert_eq!(legacy.encode_state().unwrap(), old);
                    let mut reset = restored.clone();
                    reset.reset();
                    assert_eq!(&reset.encode_state().unwrap()[8..12], &1_u32.to_le_bytes());
                    assert_eq!(reset.dump_save_ram(), Some(vec![0x47; size]));
                    machine.drain_audio_samples();
                    restored.drain_audio_samples();
                    for phase in [4, 5] {
                        run_to_phase(&mut machine, phase);
                        run_to_phase(&mut restored, phase);
                        let mut cells = vec![0x47; size];
                        if phase == 5 {
                            cells[..2].copy_from_slice(&[0x22, 0x44]);
                        }
                        assert_eq!(machine.dump_save_ram(), Some(cells.clone()));
                        assert_eq!(restored.dump_save_ram(), Some(cells));
                        assert_eq!(machine.peek(0xff0000), Some(0x47));
                        assert_eq!(
                            machine.encode_state().unwrap(),
                            restored.encode_state().unwrap()
                        );
                    }
                    assert_eq!(
                        &machine.encode_state().unwrap()[8..12],
                        &1_u32.to_le_bytes()
                    );
                    for _ in 0..3000 {
                        assert_eq!(machine.step_instruction(), restored.step_instruction());
                    }
                    assert_eq!(
                        machine.encode_state().unwrap(),
                        restored.encode_state().unwrap()
                    );
                    assert_eq!(machine.framebuffer(), restored.framebuffer());
                    let audio = machine.drain_audio_samples();
                    assert!(audio.iter().any(|sample| *sample != 0.0));
                    assert_eq!(audio, restored.drain_audio_samples());
                }
            }
        }
    }
}

#[test]
fn legacy_512_disabled_mapping_is_still_a_valid_state() {
    let mut machine = fixture(512, Region::NtscUs, true);
    let cells = machine.dump_save_ram();
    let mut disabled = machine.encode_state().unwrap();
    let enabled = crate::state_layout::save_range(&disabled).start + 1;
    disabled[enabled] = 0;
    machine.decode_state(&disabled).unwrap();
    assert_eq!(machine.encode_state().unwrap(), disabled);
    assert_eq!(machine.dump_save_ram(), cells);
    assert_eq!(machine.peek(0x200001), None);
    assert_eq!(machine.peek(0x2003ff), None);
}
