#[path = "save_capacity/fixture.rs"]
mod fixture;
#[path = "support/state_layout.rs"]
mod state_layout;

use fixture::{Rom, all_cells_rom, config, expected, run_to_phase};
use sha2::{Digest, Sha256};
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_md_core::{Emulator, LoadConfig, LoadError, Region};

fn configs() -> impl Iterator<Item = LoadConfig> {
    [Region::NtscUs, Region::NtscJapan, Region::Pal]
        .into_iter()
        .flat_map(|region| [false, true].map(move |concurrent| config(region, concurrent)))
}

fn profile(mut bytes: Vec<u8>, product: &[u8]) -> Vec<u8> {
    bytes.resize(0x300000, 0x69);
    bytes[0x180..0x18e].copy_from_slice(product);
    bytes
}

fn admitted(bytes: Vec<u8>, settings: LoadConfig) -> Emulator {
    Emulator::new(profile(bytes, b"GM 00001022-00"), settings).unwrap()
}

fn normalize_rom_identity(mut raw: Vec<u8>) -> Vec<u8> {
    assert_eq!(&raw[..8], b"ZMDRAW01");
    assert_eq!(&raw[8..12], &1_u32.to_le_bytes());
    raw[12..44].fill(0);
    raw
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn witness(line: String) {
    #[cfg(not(target_arch = "wasm32"))]
    println!("{line}");
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("{}", line);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn overlap32_admission_requires_full_product_capacity_and_size() {
    let bytes = profile(Rom::new(32768).finish(), b"GM 00001022-00");
    let settings = config(Region::NtscJapan, true);
    let machine = Emulator::new(bytes.clone(), settings).unwrap();
    assert_eq!(machine.rom_bytes(), bytes);
    assert_eq!(machine.dump_save_ram(), Some(vec![0xff; 32768]));
    assert_eq!(machine.peek(0x200001), Some(0x69));
    for offset in 0x1b0..0x1b2 {
        for bit in 0..8 {
            let mut wrong = bytes.clone();
            wrong[offset] ^= 1 << bit;
            if let Ok(machine) = Emulator::new(wrong, settings) {
                assert_eq!(machine.dump_save_ram(), None);
            }
        }
    }
    for offset in (0x180..0x18e).chain(0x1b2..0x1bc) {
        for bit in 0..8 {
            let mut wrong = bytes.clone();
            wrong[offset] ^= 1 << bit;
            assert!(
                matches!(
                    Emulator::new(wrong, settings),
                    Err(LoadError::UnsupportedSaveHardware)
                ),
                "offset={offset:x} bit={bit}"
            );
        }
    }
    for size in [0x200002, 0x280000, 0x2fffff, 0x300001, 0x400000] {
        let mut wrong = bytes.clone();
        wrong.resize(size, 0);
        assert!(matches!(
            Emulator::new(wrong, settings),
            Err(LoadError::UnsupportedSaveHardware)
        ));
    }
    for product in [b"GM MK-1239 -00", b"GM MK-1242 -00", b"GM MK-1244 -00"] {
        let legacy = profile(Rom::new(32768).finish(), product);
        assert!(Emulator::new(legacy, settings).is_ok());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn overlap32_guest_fills_and_reads_all32768_cells_twice() {
    for settings in configs() {
        let mut machine = admitted(all_cells_rom(32768), settings);
        for (phase, seed) in [(1, 0x19), (2, 0xe6)] {
            run_to_phase(&mut machine, phase);
            let cells = expected(32768, seed);
            assert_eq!(machine.dump_save_ram(), Some(cells.clone()));
            for (index, value) in cells.into_iter().enumerate() {
                assert_eq!(machine.peek(0xff0000 + index as u32), Some(value));
                assert_eq!(machine.peek(0x200001 + index as u32 * 2), Some(value));
                assert_eq!(machine.peek(0x200000 + index as u32 * 2), Some(0xff));
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn overlap32_mapping_persistence_lanes_and_reset_preserve_cells() {
    let mut rom = Rom::new(32768);
    rom.byte(0x200001, 0x45);
    rom.read_byte(0x200001, 0xff0000);
    rom.enable();
    rom.byte(0x200001, 0x12);
    rom.byte(0x20ffff, 0xa7);
    rom.byte(0x200000, 0x99);
    rom.write_word(0x200002, 0x3456);
    rom.read_byte(0x200001, 0xff0001);
    rom.read_byte(0x20ffff, 0xff0002);
    rom.phase(1);
    rom.byte(0xa130f1, 0);
    rom.byte(0x200001, 0x66);
    rom.read_byte(0x200001, 0xff0003);
    rom.phase(2);
    rom.enable();
    rom.read_byte(0x200003, 0xff0004);
    rom.word(0x4e70);
    let bytes = rom.finish();
    for settings in configs() {
        let mut machine = admitted(bytes.clone(), settings);
        let preload = expected(32768, 0x37);
        machine.load_save_ram(&preload).unwrap();
        assert_eq!(machine.peek(0x200001), Some(0x69));
        for phase in [1, 2] {
            run_to_phase(&mut machine, phase);
            let cells = machine.dump_save_ram().unwrap();
            assert_eq!(cells[0], 0x12);
            assert_eq!(cells[1], 0x56);
            assert_eq!(cells[32767], 0xa7);
            assert_eq!(&cells[2..32767], &preload[2..32767]);
            assert_eq!(machine.peek(0xff0000), Some(0x69));
            assert_eq!(machine.peek(0xff0001), Some(0x12));
            assert_eq!(machine.peek(0xff0002), Some(0xa7));
            assert_eq!(
                machine.peek(0x200001),
                Some(if phase == 1 { 0x12 } else { 0x69 })
            );
            if phase == 2 {
                assert_eq!(machine.peek(0xff0003), Some(0x69));
            }
            for length in [0, 8192, 32767, 32769] {
                let before = machine.clone();
                assert!(machine.load_save_ram(&vec![0; length]).is_err());
                assert_eq!(machine, before);
            }
        }
        run_to_phase(&mut machine, 3);
        assert_eq!(machine.peek(0xff0004), Some(0x56));
        assert_eq!(machine.reset_pulses(), 1);
        let cells = machine.dump_save_ram();
        machine.reset_cpu().unwrap();
        assert_eq!(machine.dump_save_ram(), cells);
        assert_eq!(machine.peek(0x200001), Some(0x12));
        machine.reset();
        assert_eq!(machine.dump_save_ram(), cells);
        assert_eq!(machine.peek(0x200001), Some(0x69));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn overlap32_state_futures_match_existing_board_after_rom_identity() {
    let mut rom = Rom::new(32768);
    rom.psg();
    rom.phase(1);
    rom.enable();
    rom.byte(0x200001, 0x47);
    rom.byte(0x20ffff, 0xb3);
    rom.phase(2);
    rom.byte(0xa130f1, 0);
    rom.phase(4);
    let bytes = rom.finish();
    for settings in configs() {
        let mut machine = admitted(bytes.clone(), settings);
        let mut legacy =
            Emulator::new(profile(bytes.clone(), b"GM MK-1244 -00"), settings).unwrap();
        for phase in [1, 2, 4, 3] {
            run_to_phase(&mut machine, phase);
            run_to_phase(&mut legacy, phase);
            let raw = machine.encode_state().unwrap();
            assert_eq!(
                normalize_rom_identity(raw.clone()),
                normalize_rom_identity(legacy.encode_state().unwrap())
            );
            let save = state_layout::save_range(&raw);
            assert_eq!(save.len(), 32771);
            assert_eq!(raw[save.start], 1);
            assert_eq!(raw[save.start + 1], u8::from(phase == 2));
            let frontend = machine.encode_frontend_state().unwrap();
            let mut futures = Vec::new();
            for front in [false, true] {
                let mut restored = admitted(bytes.clone(), settings);
                let mut control = machine.clone();
                if front {
                    restored.decode_frontend_state(&frontend).unwrap();
                } else {
                    restored.decode_state(&raw).unwrap();
                    control.decode_state(&raw).unwrap();
                }
                assert_eq!(restored.encode_state().unwrap(), raw);
                assert_eq!(
                    restored.encode_frontend_state().unwrap(),
                    control.encode_frontend_state().unwrap()
                );
                for _ in 0..128 {
                    assert_eq!(control.step_instruction(), restored.step_instruction());
                }
                control.step_field().unwrap();
                restored.step_field().unwrap();
                assert_eq!(
                    control.encode_state().unwrap(),
                    restored.encode_state().unwrap()
                );
                assert_eq!(control.framebuffer(), restored.framebuffer());
                let pcm = control.drain_audio_samples();
                assert!(!pcm.is_empty() && pcm.iter().any(|x| *x != 0.0));
                assert_eq!(pcm, restored.drain_audio_samples());
                assert_eq!(control.timing_snapshot(), restored.timing_snapshot());
                let pcm_bytes: Vec<_> =
                    pcm.iter().flat_map(|x| x.to_bits().to_le_bytes()).collect();
                futures.push(format!("{{\"frontend\":{front},\"rawSha256\":\"{}\",\"pcmSha256\":\"{}\",\"rgbaSha256\":\"{}\"}}", sha(&control.encode_state().unwrap()), sha(&pcm_bytes), sha(control.framebuffer())));
                let before = restored.clone();
                for end in [12, save.start, save.start + 1, raw.len() - 1] {
                    assert!(restored.decode_state(&raw[..end]).is_err());
                    assert_eq!(restored, before);
                }
                let mut malformed = vec![legacy.encode_state().unwrap()];
                for offset in [save.start, save.start + 1] {
                    let mut invalid = raw.clone();
                    invalid[offset] = 2;
                    malformed.push(invalid);
                }
                for length in [32770u32, 32772] {
                    let mut invalid = raw.clone();
                    invalid[save.start - 4..save.start].copy_from_slice(&length.to_le_bytes());
                    malformed.push(invalid);
                }
                let mut trailing = raw.clone();
                trailing.push(0);
                malformed.push(trailing);
                for invalid in malformed {
                    assert!(restored.decode_state(&invalid).is_err());
                    assert_eq!(restored, before);
                }
            }
            witness(format!(
                "OVERLAP32_WITNESS {{\"region\":\"{:?}\",\"concurrent\":{},\"phase\":{phase},\"rawSha256\":\"{}\",\"frontendSha256\":\"{}\",\"legacyNormalizedRawSha256\":\"{}\",\"futures\":[{}]}}",
                settings.region,
                settings.concurrent_sound,
                sha(&raw),
                sha(&frontend),
                sha(&normalize_rom_identity(raw.clone())),
                futures.join(",")
            ));
        }
    }
}
