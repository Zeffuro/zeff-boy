use super::fixture::{Rom, all_cells_rom, config, expected, run_to_phase};
use zeff_emu_common::time::Reset;
use zeff_md_core::{Emulator, Region};

fn profile(mut bytes: Vec<u8>, size: usize) -> Vec<u8> {
    if size == 32768 {
        bytes.resize(0x200000, 0);
        bytes[0x180..0x18e].copy_from_slice(b"GM ACLD012 -00");
        bytes[0x18e..0x190].copy_from_slice(&[0xd7, 0x23]);
        bytes[0x1b0..0x1bc].fill(b' ');
        return bytes;
    }
    bytes.resize(0xa0000, 0);
    bytes[0x180..0x18e].copy_from_slice(b"GM T-26013 -00");
    bytes[0x1b0..0x1bc].copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0x20, 0, 0, 0x20, 0, 1, 0, 0x20]);
    bytes
}

fn excludes_profile(bytes: Vec<u8>) {
    if let Ok(machine) = Emulator::new(bytes, config(Region::NtscUs, true)) {
        assert_eq!(machine.dump_save_ram(), None);
    }
}

fn mix(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *hash = (*hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
}

pub fn profile_cases() -> u64 {
    let mut hash = 0xcbf29ce484222325;
    for size in [8192, 32768] {
        let bytes = profile(all_cells_rom(size), size);
        for offset in (0x180..if size == 32768 { 0x190 } else { 0x18e }).chain(0x1b0..0x1bc) {
            for bit in 0..8 {
                let mut malformed = bytes.clone();
                malformed[offset] ^= 1 << bit;
                excludes_profile(malformed);
            }
        }
        for length in [bytes.len() - 1, bytes.len() + 1, 0x100000, 0x300000] {
            let mut wrong_size = bytes.clone();
            wrong_size.resize(length, 0);
            excludes_profile(wrong_size);
        }
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for concurrent in [false, true] {
                let settings = config(region, concurrent);
                let mut machine = Emulator::new(bytes.clone(), settings).unwrap();
                assert_eq!(machine.rom_bytes(), bytes);
                assert_eq!(machine.dump_save_ram(), Some(vec![0xff; size]));
                assert_eq!(machine.peek(0x200001), Some(0xff));
                assert_eq!(
                    machine.check_warm_reset(),
                    Err(zeff_md_core::WarmResetRejection::CartridgeOwner)
                );
                for (phase, seed) in [(1, 0x19), (2, 0xe6)] {
                    run_to_phase(&mut machine, phase);
                    let cells = expected(size, seed);
                    assert_eq!(machine.dump_save_ram(), Some(cells.clone()));
                    for (index, &value) in cells.iter().enumerate() {
                        assert_eq!(machine.peek(0x200001 + index as u32 * 2), Some(value));
                        assert_eq!(machine.peek(0x200000 + index as u32 * 2), Some(0xff));
                        assert_eq!(machine.peek(0xff0000 + index as u32), Some(value));
                    }
                    mix(&mut hash, &cells);
                }
                machine.drain_audio_samples();
                let raw = machine.encode_state().unwrap();
                let front = machine.encode_frontend_state().unwrap();
                let save = crate::state_layout::save_range(&raw);
                let mut old_board = raw.clone();
                old_board[save.start - 4..save.start].copy_from_slice(&1_u32.to_le_bytes());
                old_board.splice(save, [0]);
                let before = machine.clone();
                assert!(machine.decode_state(&old_board).is_err());
                assert_eq!(machine, before);
                assert_eq!(&raw[8..12], &1_u32.to_le_bytes());
                mix(&mut hash, &raw);
                mix(&mut hash, &front);
                for frontend in [false, true] {
                    let mut restored = Emulator::new(bytes.clone(), settings).unwrap();
                    let mut control = machine.clone();
                    if frontend {
                        restored.decode_frontend_state(&front).unwrap();
                        assert_eq!(restored.encode_frontend_state().unwrap(), front);
                    } else {
                        restored.decode_state(&raw).unwrap();
                        control.decode_state(&raw).unwrap();
                    }
                    assert_eq!(restored.encode_state().unwrap(), raw);
                    assert_eq!(
                        restored.encode_frontend_state().unwrap(),
                        control.encode_frontend_state().unwrap()
                    );
                    for _ in 0..512 {
                        assert_eq!(control.step_instruction(), restored.step_instruction());
                    }
                    let future = control.encode_state().unwrap();
                    assert_eq!(future, restored.encode_state().unwrap());
                    assert_eq!(control.framebuffer(), restored.framebuffer());
                    let audio = control.drain_audio_samples();
                    assert_eq!(audio, restored.drain_audio_samples());
                    mix(&mut hash, &future);
                    mix(&mut hash, control.framebuffer());
                    for sample in audio {
                        mix(&mut hash, &sample.to_bits().to_le_bytes());
                    }
                }
                let before = machine.clone();
                for length in [
                    0,
                    size - 1,
                    size + 1,
                    if size == 8192 { 32768 } else { 8192 },
                ] {
                    assert!(machine.load_save_ram(&vec![0x37; length]).is_err());
                    assert_eq!(machine, before);
                }
                let cells = machine.dump_save_ram();
                machine.reset();
                assert_eq!(machine.dump_save_ram(), cells);
                assert_eq!(machine.peek(0x200001), Some(expected(size, 0xe6)[0]));
                let mut rom = Rom::new(size);
                let last = 0x200001 + size as u32 * 2 - 2;
                rom.word(0x4639);
                rom.long(last);
                rom.read_byte(last, 0xff0004);
                rom.word(0x4639);
                rom.long(last);
                rom.read_byte(last, 0xff0005);
                rom.write_word(0x200000, 0x1234);
                rom.write_long(0x200000 + size as u32 * 2 - 2, 0x56789abc);
                rom.byte(0x200000, 0x99);
                if size == 32768 {
                    rom.byte(0x1ef501, 0x99);
                }
                rom.read_byte(0x200001 + size as u32 * 2, 0xff0000);
                let lane_bytes = profile(rom.finish(), size);
                let mut lanes = Emulator::new(lane_bytes, settings).unwrap();
                run_to_phase(&mut lanes, 3);
                let saved = lanes.dump_save_ram().unwrap();
                assert_eq!(saved[0], 0x34);
                assert_eq!(saved[size - 1], 0x78);
                assert!(saved[1..size - 1].iter().all(|&v| v == 0xff));
                assert_eq!(lanes.peek(0xff0000), Some(0xff));
                assert_eq!(lanes.peek(0xff0004), Some(0));
                assert_eq!(lanes.peek(0xff0005), Some(0xff));
                assert_eq!(lanes.peek(0x200000), Some(0xff));
                assert_eq!(lanes.peek(0x200001 + size as u32 * 2 - 2), Some(0x78));
                if size == 32768 {
                    assert_eq!(lanes.peek(0x1ef501), Some(0));
                }
                mix(&mut hash, &saved);
            }
        }
    }
    hash
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn corrected_odd8_profile_preserves_guest_cells_lanes_identity_and_continuations() {
    let digest = profile_cases();
    println!("PROFILE79_DIGEST:{digest:016x}");
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("PROFILE79_DIGEST:{:016x}", digest);
    assert_ne!(digest, 0);
}
