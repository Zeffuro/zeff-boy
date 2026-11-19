use super::*;
use crate::emulator::{LoadConfig, Region, TimingModel};
use std::sync::Arc;
use zeff_emu_common::cheats::CheatValue;
use zeff_emu_common::time::Reset;

mod download_tests;
mod fixed_register_tests;
mod linear_tests;
mod lock_on_tests;
mod mapper_tests;
mod parallel_tests;
mod realtec_tests;
mod save_latch_tests;
mod save_tests;
mod sf001_tests;
mod sound_tests;
mod special_save_tests;
mod state_tests;
mod tmss_tests;
mod unprotected_tests;

const DATA: u32 = 0x18000;
const CODE: u32 = 0x10100;
const ROM_LEN: usize = 0x20000;

fn write_words(image: &mut [u8], offset: usize, words: &[u16]) {
    for (index, word) in words.iter().enumerate() {
        image[offset + index * 2..offset + index * 2 + 2].copy_from_slice(&word.to_be_bytes());
    }
}

fn image(program: &[u16]) -> Vec<u8> {
    let mut image = vec![0; ROM_LEN];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    write_words(&mut image, 0x200, program);
    write_words(
        &mut image,
        CODE as usize,
        &[0x7001, 0x0640, 3, 0x60f8, 0x4e71],
    );
    image[DATA as usize..DATA as usize + 4].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
    image
}

fn fixture(program: &[u16], timing: TimingModel, region: Region) -> Emulator {
    Emulator::new_with_timing(
        image(program),
        LoadConfig {
            region,
            concurrent_sound: true,
        },
        timing,
    )
    .unwrap()
}

fn patch(address: u32, value: u8) -> CheatPatch {
    CheatPatch::WideRomWrite {
        address,
        value: CheatValue::Constant(value),
    }
}

fn patches(text: &str) -> Vec<CheatPatch> {
    if text.is_empty() {
        return Vec::new();
    }
    let (patches, _) = parse_cheats(text).unwrap();
    patches
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn guest_byte_word_opcode_and_operand_reads_use_physical_high_offsets() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut machine = fixture(
                &[
                    0x1039, 1, 0x8000, 0x1239, 1, 0x8001, 0x3439, 1, 0x8000, 0x60fe,
                ],
                timing,
                region,
            );
            assert!(machine.supports_rom_cheats());
            machine
                .install_rom_patches(&patches("018000:A55A"))
                .unwrap();
            assert_eq!(machine.peek(DATA), Some(0xa5));
            assert_eq!(machine.peek(DATA + 1), Some(0x5a));
            for _ in 0..3 {
                machine.step_instruction().unwrap();
            }
            assert_eq!(machine.registers().d[0] & 0xff, 0xa5);
            assert_eq!(machine.registers().d[1] & 0xff, 0x5a);
            assert_eq!(machine.registers().d[2] & 0xffff, 0xa55a);

            let mut machine = fixture(&[0x4ef9, 1, 0x0100, 0x4e71], timing, region);
            machine
                .install_rom_patches(&patches("010100:7007 + 010104:4567"))
                .unwrap();
            machine.step_instruction().unwrap();
            assert_eq!(machine.queued_words(), Some([0x7007, 0x0640]));
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().d[0], 7);
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().d[0], 0x456e);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn overlay_is_external_immutable_and_compares_each_patch_to_original_source() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut machine = fixture(&[0x60fe, 0x4e71], timing, Region::NtscUs);
        let source = machine.bus.cartridge.bytes.clone();
        let hash = machine.bus.cartridge.rom_digest();
        let header = machine.bus.cartridge.header.clone();
        let initial = machine.bus.cartridge.initial;
        let saved = machine.encode_frontend_state().unwrap();
        machine
            .install_rom_patches(&[
                patch(DATA, 0xa5),
                CheatPatch::WideRomWriteIfEquals {
                    address: DATA,
                    value: CheatValue::Constant(0x5a),
                    compare: CheatValue::Constant(0x12),
                },
                CheatPatch::WideRomWriteIfEquals {
                    address: DATA,
                    value: CheatValue::Constant(0xee),
                    compare: CheatValue::Constant(0xa5),
                },
                CheatPatch::WideRomWrite {
                    address: DATA + 1,
                    value: CheatValue::PreserveWithCurrent {
                        mask: 0xf0,
                        base: 0x0b,
                    },
                },
                CheatPatch::RomWrite {
                    address: 0x40,
                    value: CheatValue::Constant(0x99),
                },
            ])
            .unwrap();
        assert!(Arc::ptr_eq(&source, &machine.bus.cartridge.bytes));
        assert_eq!(machine.bus.cartridge.bytes.as_ref(), source.as_ref());
        assert_eq!(machine.bus.cartridge.rom_digest(), hash);
        assert_eq!(machine.bus.cartridge.header, header);
        assert_eq!(machine.bus.cartridge.initial, initial);
        assert_eq!(machine.peek(DATA), Some(0x5a));
        assert_eq!(machine.peek(DATA + 1), Some(0x3b));
        assert_eq!(machine.peek(0x40), Some(0x99));
        assert_eq!(machine.encode_frontend_state().unwrap(), saved);
        machine.clear_rom_patches();
        assert_eq!(machine.peek(DATA), Some(0x12));
        assert_eq!(machine.encode_frontend_state().unwrap(), saved);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn source_boundaries_and_rejected_lists_are_atomic() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut machine = fixture(&[0x60fe, 0x4e71], timing, Region::NtscUs);
        machine
            .install_rom_patches(&patches("01FFFF:A5 + 018000:5A"))
            .unwrap();
        assert_eq!(machine.peek((ROM_LEN - 1) as u32), Some(0xa5));
        let before = machine.clone();
        for address in [ROM_LEN as u32, 0x3fffff, 0x400000, 0x1000000, u32::MAX] {
            assert!(
                machine
                    .install_rom_patches(&[patch(DATA, 0xff), patch(address, 1)])
                    .is_err()
            );
            assert_eq!(machine, before);
        }
        assert!(
            machine
                .install_rom_patches(&patches("01FFFF:A55A"))
                .is_err()
        );
        assert_eq!(machine, before);
        machine.install_rom_patches(&[patch(DATA, 0x12)]).unwrap();
        assert!(machine.bus.cartridge.rom_overlay.is_empty());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn effective_vectors_and_first_prefetch_survive_clone_and_cold_reset() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut machine = fixture(&[0x60fe, 0x4e71], timing, region);
            let initial = machine.bus.cartridge.initial;
            machine
                .install_rom_patches(&patches(
                    "000000:1234 + 000002:5679 + 000004:8001 + 000006:0100 + 010100:7009",
                ))
                .unwrap();
            assert_eq!(machine.bus.cartridge.initial, initial);
            assert_eq!(machine.bus.cartridge.effective_initial().pc, 0x80010100);
            assert_eq!(machine.bus.cartridge.effective_initial().ssp, 0x12345679);
            let mut clone = machine.clone();
            clone.clear_rom_patches();
            assert_eq!(machine.peek(CODE + 1), Some(9));
            assert_eq!(clone.peek(CODE + 1), Some(1));
            machine.reset();
            assert_eq!(machine.registers().pc, 0x80010100);
            assert_eq!(machine.registers().ssp, 0x12345679);
            assert_eq!(machine.queued_words(), Some([0x7009, 0x0640]));
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().d[0], 9);
            if timing == TimingModel::Standard {
                machine.reset_cpu().unwrap();
                assert_eq!(machine.queued_words(), Some([0x7009, 0x0640]));
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn invalid_final_reset_vectors_reject_without_mutating_live_samples() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut machine = fixture(&[0x60fe, 0x4e71], timing, Region::NtscUs);
        machine.install_rom_patches(&[patch(DATA, 0xa5)]).unwrap();
        let before = machine.clone();
        for text in [
            "018000:55 + 000007:01",
            "018000:55 + 000004:0002 + 000006:0000",
            "018000:55 + 000004:0001 + 000006:FFFE",
        ] {
            assert!(
                machine.install_rom_patches(&patches(text)).is_err(),
                "{text}"
            );
            assert_eq!(machine, before);
        }
        machine
            .install_rom_patches(&patches("000007:01 + 000007:00 + 018000:55"))
            .unwrap();
        assert_eq!(machine.peek(DATA), Some(0x55));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn trace_changes_follow_effective_sets_and_rejection_happens_before_reset() {
    let mut machine = fixture(&[0x60fe, 0x4e71], TimingModel::Standard, Region::NtscUs);
    machine.reset_and_begin_audio_trace(32).unwrap();
    machine.install_rom_patches(&[patch(DATA, 0x12)]).unwrap();
    machine
        .install_rom_patches(&[CheatPatch::WideRomWriteIfEquals {
            address: DATA,
            value: CheatValue::Constant(0xa5),
            compare: CheatValue::Constant(0xee),
        }])
        .unwrap();
    assert_eq!(
        machine.finish_audio_trace().unwrap().trace.invalidated,
        None
    );
    machine.reset_and_begin_audio_trace(32).unwrap();
    machine.install_rom_patches(&[patch(DATA, 0xa5)]).unwrap();
    let before = machine.clone();
    machine
        .install_rom_patches(&[patch(DATA, 0), patch(DATA, 0xa5)])
        .unwrap();
    assert_eq!(machine, before);
    assert!(machine.reset_and_begin_audio_trace(32).is_err());
    assert_eq!(machine, before);
    assert_eq!(
        machine.finish_audio_trace().unwrap().trace.invalidated,
        Some(AudioTraceInvalidation::ExternalMutation)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn ordinary_save_admits_rom_cheats() {
    let mut rom = image(&[0x60fe, 0x4e71]);
    rom[0x1b0..0x1bc]
        .copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20, 0x3f, 0xff]);
    let mut save = Emulator::new(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
    )
    .unwrap();
    assert!(save.bus.cartridge.save.is_some());
    assert!(save.supports_rom_cheats());
    save.install_rom_patches(&[patch(DATA, 0xa5)]).unwrap();
    assert_eq!(save.peek(DATA), Some(0xa5));
}
