use super::*;
use crate::cartridge::parallel_adapter::{Model, ParallelAdapter};

fn changed_backing(config: LoadConfig) -> Emulator {
    let mut machine = Emulator::new(
        download_image(&[0x13fc, 0, 0x00a1, 0x30f1, 0x33fc, 0xa55a, 0, 0x1234, 0x60fe]),
        config,
    )
    .unwrap();
    machine.step_instruction().unwrap();
    machine.step_instruction().unwrap();
    assert_eq!(machine.peek(0x1234), Some(0xa5));
    assert_eq!(machine.peek(0x1235), Some(0x5a));
    machine
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_preparation_uses_original_bytes_and_retains_equal_masks() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            let mut machine = changed_backing(LoadConfig {
                region,
                concurrent_sound,
            });
            let original = machine.bus.cartridge.bytes.clone();
            let digest = machine.bus.cartridge.rom_digest();
            let header = machine.bus.cartridge.header.clone();
            let initial = machine.bus.cartridge.initial;
            let raw = machine.encode_state().unwrap();
            let set = [
                patch(0, 0x12),
                patch(0x2fffff, 0x88),
                patch(0x1234, 0xa5),
                CheatPatch::WideRomWriteIfEquals {
                    address: 0x1234,
                    value: CheatValue::Constant(0x5a),
                    compare: CheatValue::Constant(0x67),
                },
                CheatPatch::WideRomWriteIfEquals {
                    address: 0x1234,
                    value: CheatValue::Constant(0xee),
                    compare: CheatValue::Constant(0xa5),
                },
                patch(0x1236, 0xa5),
                CheatPatch::WideRomWrite {
                    address: 0x1236,
                    value: CheatValue::PreserveWithCurrent {
                        mask: 0xf0,
                        base: 0x0b,
                    },
                },
                patch(0x1235, 0),
            ];
            machine.install_rom_patches(&set).unwrap();
            assert_eq!(machine.peek(0), Some(0x12));
            assert_eq!(machine.peek(0x2fffff), Some(0x88));
            assert_eq!(machine.peek(0x300000), Some(0xff));
            assert_eq!(machine.peek(0x1234), Some(0x5a));
            assert_eq!(machine.peek(0x1235), Some(0));
            assert_eq!(machine.peek(0x1236), Some(0x0b));
            assert_eq!(machine.encode_state().unwrap(), raw);
            let before = machine.clone();
            machine.install_rom_patches(&set).unwrap();
            assert_eq!(machine, before);
            assert!(Arc::ptr_eq(&original, &machine.bus.cartridge.bytes));
            assert_eq!(machine.bus.cartridge.bytes.as_ref(), original.as_ref());
            assert_eq!(machine.bus.cartridge.rom_digest(), digest);
            assert_eq!(machine.bus.cartridge.header, header);
            assert_eq!(machine.bus.cartridge.initial, initial);
            machine.clear_rom_patches();
            assert_eq!(machine.peek(0x1234), Some(0xa5));
            assert_eq!(machine.peek(0x1235), Some(0x5a));
            assert_eq!(machine.peek(0x1236), Some(0));
            assert_eq!(machine.peek(0), Some(0));
            assert_eq!(machine.peek(0x2fffff), Some(0));
            assert_eq!(machine.encode_state().unwrap(), raw);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_rejected_lists_and_reset_vectors_preserve_entire_machine() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            let mut machine = changed_backing(LoadConfig {
                region,
                concurrent_sound,
            });
            machine.install_rom_patches(&[patch(0x1234, 0x67)]).unwrap();
            let before = machine.clone();
            for address in [0x300000, 0x3fffff, 0x400000, 0x1000000, u32::MAX] {
                assert!(
                    machine
                        .install_rom_patches(&[patch(0x1234, 0xcc), patch(address, 1)])
                        .is_err()
                );
                assert_eq!(machine, before);
                assert!(
                    machine
                        .install_rom_patches(&[patch(address, 1), patch(0x1234, 0xcc)])
                        .is_err()
                );
                assert_eq!(machine, before);
            }
            for text in [
                "001234:CC + 2FFFFF:A55A",
                "001234:CC + 000007:01",
                "001234:CC + 000004:0030 + 000006:0000",
                "001234:CC + 000004:002F + 000006:FFFE",
            ] {
                assert!(
                    machine.install_rom_patches(&patches(text)).is_err(),
                    "{text}"
                );
                assert_eq!(machine, before);
            }
            machine
                .install_rom_patches(&patches("000007:01 + 000007:00 + 001234:55"))
                .unwrap();
            assert_eq!(machine.peek(0x1234), Some(0x55));
            assert_eq!(machine.bus.cartridge.effective_initial().pc, 0x50000);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_mask_authority_rejects_stripped_injected_conflicting_and_detached_owners() {
    let bytes = download_image(&[0x60fe, 0x4e71]);
    let mut plain = bytes.clone();
    plain[0x80..0x90].fill(0);
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            let config = LoadConfig {
                region,
                concurrent_sound,
            };
            let factory = Emulator::new(bytes.clone(), config).unwrap();
            let mut stripped = factory.clone();
            stripped.bus.cartridge.download = None;
            let mut injected = Emulator::new(plain.clone(), config).unwrap();
            assert!(injected.bus.cartridge.download.is_none());
            injected.bus.cartridge.download = factory.bus.cartridge.download.clone();
            let mut conflicting = factory.clone();
            conflicting.bus.cartridge.parallel_adapter = Some(ParallelAdapter::new(Model::First));
            let mut detached = factory.clone();
            detached.bus.cartridge.bytes = bytes.clone().into();
            assert_eq!(detached.bus.cartridge.bytes, factory.bus.cartridge.bytes);
            assert!(!Arc::ptr_eq(
                &detached.bus.cartridge.bytes,
                &factory.bus.cartridge.bytes
            ));
            assert_eq!(
                detached.bus.cartridge.rom_digest(),
                factory.bus.cartridge.rom_digest()
            );
            for mut machine in [stripped, injected, conflicting, detached] {
                assert!(!machine.supports_rom_cheats());
                assert!(!machine.supports_rom_breakpoints());
                let before = machine.clone();
                assert!(machine.install_rom_patches(&[patch(0x1234, 0x88)]).is_err());
                assert_eq!(machine, before);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_cold_reset_reseeds_original_backing_and_retains_effective_fetch() {
    let mut bytes = download_image(&[
        0x13fc, 0, 0x00a1, 0x30f1, 0x33fc, 0xa55a, 0, 0x1234, 0x33fc, 0x7788, 0x0030, 0, 0x60fe,
    ]);
    write_words(
        &mut bytes,
        0x60000,
        &[0x7001, 0x33fc, 0xccdd, 0, 0x1234, 0x60fe],
    );
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            let config = LoadConfig {
                region,
                concurrent_sound,
            };
            let fresh = Emulator::new(bytes.clone(), config).unwrap();
            let mut machine = fresh.clone();
            let original = machine.bus.cartridge.bytes.clone();
            let digest = machine.bus.cartridge.rom_digest();
            for _ in 0..3 {
                machine.step_instruction().unwrap();
            }
            assert_eq!(machine.peek(0x1234), Some(0xa5));
            assert_eq!(machine.peek(0x300000), Some(0x77));
            machine
                .install_rom_patches(&patches(
                    "000002:FF00 + 000004:0006 + 000006:0000 + 060000:7009 + 001234:67",
                ))
                .unwrap();
            let before = machine.clone();
            assert!(machine.reset_cpu().is_err());
            assert_eq!(machine, before);
            assert!(machine.warm_reset().is_err());
            assert_eq!(machine, before);
            machine.reset();
            assert_eq!(machine.bus.cartridge.download, fresh.bus.cartridge.download);
            assert_eq!(
                machine.bus.cartridge.rom_overlay,
                before.bus.cartridge.rom_overlay
            );
            assert_eq!(machine.registers().pc, 0x60000);
            assert_eq!(machine.registers().ssp, 0x00ffff00);
            assert_eq!(machine.queued_words(), Some([0x7009, 0x33fc]));
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().d[0], 9);
            machine.step_instruction().unwrap();
            machine.clear_rom_patches();
            assert_eq!(machine.peek(0x1234), Some(0x67));
            assert_eq!(machine.peek(0x1235), Some(0));
            assert_eq!(machine.peek(0x300000), Some(0xff));
            assert_eq!(machine.bus.cartridge.download, fresh.bus.cartridge.download);
            assert!(Arc::ptr_eq(&original, &machine.bus.cartridge.bytes));
            assert_eq!(machine.bus.cartridge.rom_digest(), digest);
        }
    }
}
