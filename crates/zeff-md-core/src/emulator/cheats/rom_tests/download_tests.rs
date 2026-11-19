use super::*;

mod consumed_tests;
mod equal_mask_tests;
mod pending_write_tests;
mod preparation_tests;
mod sampled_tests;
mod state_tests;

fn download_image(words: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0; 0x300000];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 5, 0, 0]);
    bytes[0x80..0x90].copy_from_slice(b"DOWNLOAD_RAM_TST");
    bytes[0x100..0x104].copy_from_slice(b"SEGA");
    bytes[0x1b0..0x1bc].fill(b' ');
    bytes[0x1234] = 0x67;
    for (index, word) in words.iter().enumerate() {
        bytes[0x50000 + index * 2..0x50002 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    bytes
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_mask_preserves_hidden_guest_writes_and_original_equal_entries() {
    let words: &[u16] = &[
        0x13fc, 0, 0x00a1, 0x30f1, 0x33fc, 0xa55a, 0, 0x1234, 0x3039, 0, 0x1234, 0x33fc, 0xccdd, 0,
        0x1234, 0x3239, 0, 0x1234, 0x3439, 0, 0x1234, 0x60fe,
    ];
    let bytes = download_image(words);
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            let mut machine = Emulator::new(
                bytes.clone(),
                LoadConfig {
                    region,
                    concurrent_sound,
                },
            )
            .unwrap();
            let original = machine.bus.cartridge.bytes.clone();
            let digest = machine.bus.cartridge.rom_digest();
            assert!(machine.supports_rom_cheats());
            assert!(!machine.supports_rom_breakpoints());
            machine.step_instruction().unwrap();
            machine.step_instruction().unwrap();
            assert_eq!(machine.peek(0x1234), Some(0xa5));
            assert_eq!(machine.peek(0x1235), Some(0x5a));
            let raw = machine.encode_state().unwrap();
            machine
                .install_rom_patches(&[patch(0x1234, 0x67), patch(0x1235, 0x80)])
                .unwrap();
            assert_eq!(machine.encode_state().unwrap(), raw);
            assert_eq!(machine.peek(0x1234), Some(0x67));
            assert_eq!(
                machine
                    .bus
                    .cartridge
                    .download
                    .as_ref()
                    .unwrap()
                    .read(0x1234),
                Some(0xa5)
            );
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().d[0] & 0xffff, 0x6780);
            machine.step_instruction().unwrap();
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().d[1] & 0xffff, 0x6780);
            assert_eq!(
                machine
                    .bus
                    .cartridge
                    .download
                    .as_ref()
                    .unwrap()
                    .read(0x1234),
                Some(0xcc)
            );
            assert_eq!(
                machine
                    .bus
                    .cartridge
                    .download
                    .as_ref()
                    .unwrap()
                    .read(0x1235),
                Some(0xdd)
            );
            machine.clear_rom_patches();
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().d[2] & 0xffff, 0xccdd);
            assert_eq!(machine.peek(0x1234), Some(0xcc));
            assert_eq!(machine.peek(0x1235), Some(0xdd));
            assert_eq!(machine.bus.cartridge.rom_source_offset(0x1234), None);
            assert_eq!(machine.bus.cartridge.rom_digest(), digest);
            assert!(Arc::ptr_eq(&original, &machine.bus.cartridge.bytes));
            assert_eq!(&original[0x1234..0x1236], &[0x67, 0]);
        }
    }
}

fn tone_image(program: &[u16]) -> Vec<u8> {
    let mut words = vec![
        0x13fc, 0x85, 0xc0, 0x11, 0x13fc, 6, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11,
    ];
    words.extend_from_slice(program);
    download_image(&words)
}

fn backing_image() -> Vec<u8> {
    tone_image(&[
        0x13fc, 0, 0xa1, 0x30f1, 0x33fc, 0xa55a, 0, 0x1234, 0x3039, 0, 0x1234, 0x33fc, 0xccdd, 0,
        0x1234, 0x3239, 0, 0x1234, 0x13fc, 1, 0xa1, 0x30f1, 0x60fe,
    ])
}

fn backing_source(bytes: &[u8], config: LoadConfig) -> Emulator {
    let mut donor = Emulator::new(bytes.to_vec(), config).unwrap();
    donor.set_sample_rate(48_000);
    donor
        .install_rom_patches(&[patch(0x1234, 0x77), patch(0x1235, 0x88)])
        .unwrap();
    for _ in 0..5 {
        donor.step_instruction().unwrap();
    }
    assert_eq!(donor.queued_words(), Some([0x3039, 0]));
    assert_eq!(
        donor.bus.cartridge.download.as_ref().unwrap().read(0x1234),
        Some(0xa5)
    );
    donor
}

fn receiver_oracle(donor: &Emulator, mask: &[CheatPatch], frontend: bool) -> Emulator {
    let mut oracle = donor.clone();
    oracle.install_rom_patches(mask).unwrap();
    if !frontend {
        // RAW restores chip state while restarting frontend output.
        oracle.set_sample_rate(oracle.sample_rate());
    }
    oracle
}

fn same_continuation(actual: &Emulator, oracle: &Emulator) {
    assert_eq!(actual, oracle);
    assert_eq!(
        actual.encode_state().unwrap(),
        oracle.encode_state().unwrap()
    );
    assert_eq!(
        actual.encode_frontend_state().unwrap(),
        oracle.encode_frontend_state().unwrap()
    );
    assert_eq!(actual.framebuffer(), oracle.framebuffer());
}

fn future_field(actual: &mut Emulator, oracle: &mut Emulator) {
    actual.step_field().unwrap();
    oracle.step_field().unwrap();
    same_continuation(actual, oracle);
    let pcm = actual.drain_audio_samples();
    assert!(!pcm.is_empty() && pcm.iter().any(|sample| *sample != 0.0));
    assert!(
        pcm.iter().map(|sample| sample.to_bits()).eq(oracle
            .drain_audio_samples()
            .iter()
            .map(|sample| sample.to_bits()))
    );
    same_continuation(actual, oracle);
}

fn saved_download(actual: &Emulator, donor: &Emulator, raw: &[u8]) {
    assert_eq!(actual.encode_state().unwrap(), raw);
    assert_eq!(actual.bus.cartridge.download, donor.bus.cartridge.download);
    assert_eq!(
        actual.cpu.queued_instruction_inputs(),
        donor.cpu.queued_instruction_inputs()
    );
    assert!(
        actual
            .cpu
            .queued_instruction_inputs()
            .unwrap()
            .iter()
            .all(|input| input
                .origin
                .iter()
                .all(|origin| origin.storage_offset().is_none()))
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_restores_backing_with_receiver_mask() {
    let bytes = backing_image();
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            let config = LoadConfig {
                region,
                concurrent_sound,
            };
            let donor = backing_source(&bytes, config);
            let raw = donor.encode_state().unwrap();
            let frontend = donor.encode_frontend_state().unwrap();
            let queued = donor.queued_words();
            for (mask, expected) in [
                (vec![patch(0x1234, 0x77), patch(0x1235, 0x88)], 0x7788),
                (vec![patch(0x1234, 0x67), patch(0x1235, 0x80)], 0x6780),
                (vec![], 0xa55a),
            ] {
                for use_frontend in [false, true] {
                    for fresh in [false, true] {
                        let mut receiver = if fresh {
                            Emulator::new(bytes.clone(), config).unwrap()
                        } else {
                            let mut changed = donor.clone();
                            for _ in 0..4 {
                                changed.step_instruction().unwrap();
                            }
                            assert_ne!(
                                changed.bus.cartridge.download,
                                donor.bus.cartridge.download
                            );
                            changed
                        };
                        receiver.set_sample_rate(48_000);
                        receiver.install_rom_patches(&mask).unwrap();
                        if use_frontend {
                            receiver.decode_frontend_state(&frontend).unwrap();
                            assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
                        } else {
                            receiver.decode_state(&raw).unwrap();
                        }
                        saved_download(&receiver, &donor, &raw);
                        assert_eq!(receiver.queued_words(), queued);
                        assert_eq!(
                            receiver
                                .bus
                                .cartridge
                                .download
                                .as_ref()
                                .unwrap()
                                .read(0x1234),
                            Some(0xa5)
                        );
                        assert_eq!(
                            receiver
                                .bus
                                .cartridge
                                .download
                                .as_ref()
                                .unwrap()
                                .read(0x1235),
                            Some(0x5a)
                        );
                        let mut oracle = receiver_oracle(&donor, &mask, use_frontend);
                        same_continuation(&receiver, &oracle);
                        receiver.step_instruction().unwrap();
                        oracle.step_instruction().unwrap();
                        assert_eq!(receiver.registers().d[0] & 0xffff, expected);
                        same_continuation(&receiver, &oracle);
                        let mut exposed = receiver.clone();
                        exposed.clear_rom_patches();
                        assert_eq!(exposed.peek(0x1234), Some(0xa5));
                        assert_eq!(exposed.peek(0x1235), Some(0x5a));
                        for _ in 0..3 {
                            receiver.step_instruction().unwrap();
                            oracle.step_instruction().unwrap();
                            same_continuation(&receiver, &oracle);
                        }
                        assert_eq!(
                            receiver.registers().d[1] & 0xffff,
                            if mask.is_empty() { 0xccdd } else { expected }
                        );
                        assert_eq!(
                            receiver
                                .bus
                                .cartridge
                                .download
                                .as_ref()
                                .unwrap()
                                .read(0x1234),
                            Some(0xcc)
                        );
                        assert_eq!(
                            receiver
                                .bus
                                .cartridge
                                .download
                                .as_ref()
                                .unwrap()
                                .read(0x1235),
                            Some(0xdd)
                        );
                        future_field(&mut receiver, &mut oracle);
                    }
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_restore_keeps_sampled_opcode_then_uses_receiver_mask() {
    let mut bytes = tone_image(&[0x13fc, 0, 0xa1, 0x30f1, 0x33fc, 0x7003, 6, 0, 0x4ef9, 6, 0]);
    write_words(&mut bytes, 0x60000, &[0x7001, 0x4ef9, 6, 0]);
    let original_low = bytes[0x60001];
    assert_eq!(original_low, 1);
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            let config = LoadConfig {
                region,
                concurrent_sound,
            };
            let mut donor = Emulator::new(bytes.clone(), config).unwrap();
            donor.set_sample_rate(48_000);
            donor
                .install_rom_patches(&[patch(0x60000, 0x70), patch(0x60001, 7)])
                .unwrap();
            for _ in 0..6 {
                donor.step_instruction().unwrap();
            }
            assert_eq!(donor.queued_words(), Some([0x7007, 0x4ef9]));
            assert_eq!(
                donor.bus.cartridge.download.as_ref().unwrap().read(0x60001),
                Some(3)
            );
            let raw = donor.encode_state().unwrap();
            let frontend = donor.encode_frontend_state().unwrap();
            for (mask, future) in [
                (vec![patch(0x60000, 0x70), patch(0x60001, 7)], 7),
                (vec![patch(0x60000, 0x70), patch(0x60001, 9)], 9),
                (vec![], 3),
            ] {
                for use_frontend in [false, true] {
                    for fresh in [false, true] {
                        let mut receiver = if fresh {
                            Emulator::new(bytes.clone(), config).unwrap()
                        } else {
                            let mut changed = donor.clone();
                            for _ in 0..3 {
                                changed.step_instruction().unwrap();
                            }
                            assert_ne!(changed.queued_words(), donor.queued_words());
                            changed
                        };
                        receiver.set_sample_rate(48_000);
                        receiver.install_rom_patches(&mask).unwrap();
                        if use_frontend {
                            receiver.decode_frontend_state(&frontend).unwrap();
                            assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
                        } else {
                            receiver.decode_state(&raw).unwrap();
                        }
                        saved_download(&receiver, &donor, &raw);
                        assert_eq!(receiver.queued_words(), Some([0x7007, 0x4ef9]));
                        let mut oracle = receiver_oracle(&donor, &mask, use_frontend);
                        same_continuation(&receiver, &oracle);
                        for step in 0..3 {
                            receiver.step_instruction().unwrap();
                            oracle.step_instruction().unwrap();
                            same_continuation(&receiver, &oracle);
                            if step == 0 {
                                assert_eq!(receiver.registers().d[0], 7);
                            }
                        }
                        assert_eq!(receiver.registers().d[0], future);
                        assert_eq!(receiver.bus.cartridge.rom_source_offset(0x60000), None);
                        future_field(&mut receiver, &mut oracle);
                    }
                }
            }
        }
    }
}
