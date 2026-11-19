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
        .flat_map(|region| [false, true].map(move |mode| config(region, mode)))
}

fn profile(mut bytes: Vec<u8>) -> Vec<u8> {
    bytes.resize(0x200000, 0x69);
    bytes[0x180..0x18e].copy_from_slice(b"GM T-119186-00");
    bytes[0x1b0..0x1bc].copy_from_slice(b"RA\xf8\x40\x00\x20\x00\x01\x00\x20\x0f\xff");
    bytes
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
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
fn parallel2048_requires_exact_product_size_and_declaration() {
    let bytes = profile(Rom::new(2048).finish());
    let settings = config(Region::NtscUs, true);
    let machine = Emulator::new(bytes.clone(), settings).unwrap();
    assert_eq!(machine.rom_bytes(), bytes);
    assert_eq!(machine.dump_save_ram(), Some(vec![0xff; 2048]));
    for offset in (0x180..0x18e).chain(0x1b0..0x1bc) {
        for bit in 0..8 {
            let mut wrong = bytes.clone();
            wrong[offset] ^= 1 << bit;
            if let Ok(other) = Emulator::new(wrong, settings) {
                assert_ne!(other.dump_save_ram(), Some(vec![0xff; 2048]));
            }
        }
    }
    for size in [0x100000, 0x1fffff, 0x200001, 0x300000, 0x400000] {
        let mut wrong = bytes.clone();
        wrong.resize(size, 0);
        assert!(matches!(
            Emulator::new(wrong, settings),
            Err(LoadError::UnsupportedSaveHardware)
        ));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn parallel2048_guest_writes_reads_all_cells_and_upper_aliases() {
    for settings in configs() {
        let mut machine = Emulator::new(profile(all_cells_rom(2048)), settings).unwrap();
        for (phase, seed) in [(1, 0x19), (2, 0xe6)] {
            run_to_phase(&mut machine, phase);
            let cells = expected(2048, seed);
            assert_eq!(machine.dump_save_ram(), Some(cells.clone()));
            for (index, value) in cells.iter().copied().enumerate() {
                assert_eq!(machine.peek(0xff0000 + index as u32), Some(value));
                assert_eq!(machine.peek(0x230001 + index as u32 * 2), Some(value));
                assert_eq!(machine.peek(0x230000 + index as u32 * 2), Some(0xff));
            }
            for mirror in 0..512 {
                let base = 0x200000 + mirror * 0x1000;
                assert_eq!(machine.peek(base), Some(0xff));
                assert_eq!(machine.peek(base + 1), Some(cells[0]));
                assert_eq!(machine.peek(base + 0xffe), Some(0xff));
                assert_eq!(machine.peek(base + 0xfff), Some(cells[2047]));
            }
            assert_eq!(machine.peek(0x1fffff), Some(0x69));
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn parallel2048_mirrored_guest_stores_preload_and_reset_domains() {
    let mut rom = Rom::new(2048);
    rom.byte(0x230001, 0x12);
    rom.byte(0x3fffff, 0xa7);
    rom.byte(0x230000, 0);
    rom.write_word(0x231002, 0x3456);
    rom.byte(0x200001, 0x47);
    rom.word(0x4e70);
    rom.read_byte(0x230001, 0xff0000);
    rom.read_byte(0x231003, 0xff0001);
    rom.read_byte(0x230fff, 0xff0002);
    let bytes = profile(rom.finish());
    for settings in configs() {
        let mut machine = Emulator::new(bytes.clone(), settings).unwrap();
        let preload = expected(2048, 0x84);
        machine.load_save_ram(&preload).unwrap();
        run_to_phase(&mut machine, 3);
        let cells = machine.dump_save_ram().unwrap();
        assert_eq!(cells[0], 0x47);
        assert_eq!(cells[1], 0x56);
        assert_eq!(cells[2047], 0xa7);
        assert_eq!(&cells[2..2047], &preload[2..2047]);
        for (index, value) in [0x47, 0x56, 0xa7].into_iter().enumerate() {
            assert_eq!(machine.peek(0xff0000 + index as u32), Some(value));
        }
        assert_eq!(machine.reset_pulses(), 1);
        for length in [0, 512, 2047, 2049, 8192] {
            let before = machine.clone();
            assert!(machine.load_save_ram(&vec![0; length]).is_err());
            assert_eq!(machine, before);
        }
        machine.reset_cpu().unwrap();
        assert_eq!(machine.dump_save_ram(), Some(cells.clone()));
        assert_eq!(machine.peek(0x230001), Some(0x47));
        machine.reset();
        assert_eq!(machine.dump_save_ram(), Some(cells));
        assert_eq!(machine.peek(0x230001), Some(0x47));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn parallel2048_raw_and_frontend_checkpoints_preserve_exact_futures() {
    let mut rom = Rom::new(2048);
    rom.psg();
    rom.phase(1);
    rom.byte(0x230001, 0x47);
    rom.byte(0x3fffff, 0xb3);
    rom.phase(2);
    rom.word(0x4e70);
    rom.phase(4);
    let bytes = profile(rom.finish());
    for settings in configs() {
        let mut machine = Emulator::new(bytes.clone(), settings).unwrap();
        for phase in [1, 2, 4, 3] {
            run_to_phase(&mut machine, phase);
            let raw = machine.encode_state().unwrap();
            let save = state_layout::save_range(&raw);
            assert_eq!(save.len(), 2051);
            assert_eq!(&raw[save.start..save.start + 2], &[1, 1]);
            assert_eq!(raw[save.end - 1], 0);
            assert_eq!(
                &raw[save.start + 2..save.end - 1],
                machine.dump_save_ram().unwrap().as_slice()
            );
            let frontend = machine.encode_frontend_state().unwrap();
            let mut futures = Vec::new();
            for front in [false, true] {
                let mut restored = Emulator::new(bytes.clone(), settings).unwrap();
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
                let mut malformed = Vec::new();
                for (offset, value) in [
                    (save.start, 2),
                    (save.start + 1, 0),
                    (save.start + 1, 2),
                    (save.end - 1, 1),
                    (save.end - 1, 2),
                    (12, raw[12] ^ 1),
                ] {
                    let mut invalid = raw.clone();
                    invalid[offset] = value;
                    malformed.push(invalid);
                }
                for length in [(save.len() - 1) as u32, (save.len() + 1) as u32] {
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
                "PARALLEL2048_WITNESS {{\"region\":\"{:?}\",\"concurrent\":{},\"phase\":{phase},\"rawSha256\":\"{}\",\"frontendSha256\":\"{}\",\"futures\":[{}]}}",
                settings.region,
                settings.concurrent_sound,
                sha(&raw),
                sha(&frontend),
                futures.join(",")
            ));
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn parallel2048_unqualified_disable_controls_remain_guarded() {
    use zeff_m68k::{bus::Direction, timing::TimingFault};
    use zeff_md_core::{BusError, MachineFault};

    for settings in configs() {
        for value in [0, 2] {
            let mut rom = Rom::new(2048);
            rom.byte(0xa130f1, value);
            let mut machine = Emulator::new(profile(rom.finish()), settings).unwrap();
            let cells = expected(2048, value);
            machine.load_save_ram(&cells).unwrap();
            let fault = (0..32)
                .find_map(|_| machine.step_instruction().err())
                .unwrap();
            assert!(matches!(
                fault,
                MachineFault::Main(TimingFault::Bus {
                    source: BusError::UnsupportedAccess {
                        address: 0xa130f0,
                        direction: Direction::Write
                    },
                    ..
                })
            ));
            assert_eq!(machine.dump_save_ram(), Some(cells.clone()));
            for address in [0x200001, 0x230001, 0x3ff001] {
                assert_eq!(machine.peek(address), Some(cells[0]));
            }
        }
    }
}
