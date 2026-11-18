use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf004_guest_controls_alias_main_byte_word_opcode_and_operand_reads() {
    for region in REGIONS {
        for concurrent in [false, true] {
            let mut program = Vec::new();
            write_byte(&mut program, 0xf01, 0x60);
            program.extend([0x1039, 0, 0x1000, 0x1239, 4, 0x1001, 0x3439, 0, 0x1000]);
            write_byte(&mut program, 0xf01, 0x70);
            program.extend([0x3639, 0, 0x1000, 0x60fe]);
            let mut m = machine(Profile::Sf004, &program, region, concurrent);
            m.install_rom_patches(&patches("181000:A55A")).unwrap();
            for _ in 0..6 {
                m.step_instruction().unwrap();
            }
            assert_eq!(m.registers().d[0] & 0xff, 0xa5);
            assert_eq!(m.registers().d[1] & 0xff, 0x5a);
            assert_eq!(m.registers().d[2] & 0xffff, 0xa55a);
            assert_eq!(m.registers().d[3] & 0xffff, 0x1111);
            assert_eq!(m.peek(0x1000), Some(0x11));
            select_sf004(&mut m, 6, false);
            assert_eq!(m.peek(0x1000), Some(0xa5));
            assert_eq!(m.peek(0x41000), Some(0xa5));

            let mut boot = Vec::new();
            write_byte(&mut boot, 0xf01, 0x60);
            boot.extend([0x4ef9, 0, 0x1000]);
            let mut m = machine(Profile::Sf004, &boot, region, concurrent);
            m.install_rom_patches(&patches("181000:7007 + 181004:4567"))
                .unwrap();
            for _ in 0..2 {
                m.step_instruction().unwrap();
            }
            assert_eq!(m.queued_words(), Some([0x7007, 0x0640]));
            for _ in 0..2 {
                m.step_instruction().unwrap();
            }
            assert_eq!(m.registers().d[0], 0x456e);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf004_all_banks_expansion_wrap_and_holes_map_before_overlay() {
    let mut m = machine(Profile::Sf004, &[0x60fe], Region::NtscUs, true);
    let set: Vec<_> = (0..8)
        .flat_map(|bank| {
            [
                patch(bank * 0x40000 + 0x1000, 0x80 + bank as u8),
                patch(bank * 0x40000 + 0x3ffff, 0xc0 + bank as u8),
            ]
        })
        .collect();
    m.install_rom_patches(&set).unwrap();
    for bank in 0..8 {
        for expanded in [false, true] {
            select_sf004(&mut m, bank, expanded);
            for address in [
                0x1000, 0x41000, 0x81000, 0x101000, 0x13ffff, 0x140000, 0x1fffff,
            ] {
                let offset = m
                    .bus
                    .cartridge
                    .sf004
                    .as_ref()
                    .unwrap()
                    .source_offset(address);
                let expected = offset.map(|at| {
                    if at & 0x3ffff == 0x1000 {
                        0x80 + (at >> 18) as u8
                    } else if at & 0x3ffff == 0x3ffff {
                        0xc0 + (at >> 18) as u8
                    } else {
                        m.bus.cartridge.bytes[at]
                    }
                });
                assert_eq!(
                    m.peek(address),
                    expected,
                    "bank{bank} expanded{expanded} {address:x}"
                );
            }
        }
    }
    let board = m.bus.cartridge.sf004.as_mut().unwrap();
    assert!(board.write(0xd01, 0x80));
    assert!(board.write(0x206001, 0x5a));
    assert!(board.write(0xe01, 0));
    let locked = m.bus.cartridge.sf004.clone();
    assert!(m.bus.cartridge.sf004.as_mut().unwrap().write(0xf01, 0));
    assert_eq!(m.bus.cartridge.sf004, locked);
    assert_eq!(m.peek(0x206001), Some(0x5a));
    assert_eq!(m.peek(0x216001), Some(0x5a));
    assert_eq!(m.peek(0x206000), None);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn switched_sram_guest_hides_source_patches_and_preserves_cells_on_reset() {
    for profile in [Profile::Switched8, Profile::Switched32, Profile::Time8] {
        for region in REGIONS {
            for concurrent in [false, true] {
                let mut code = Vec::new();
                write_byte(&mut code, 0xa130f1, 1);
                code.extend([0x1039, 0x20, 1]);
                write_byte(&mut code, PORT, 0x5a);
                code.extend([0x1239, 0x20, 1]);
                write_byte(&mut code, 0xa130f1, 0);
                code.extend([0x1439, 0x20, 1, 0x60fe]);
                let mut m = machine(profile, &code, region, concurrent);
                m.install_rom_patches(&[patch(PORT, 0xa5)]).unwrap();
                for _ in 0..6 {
                    m.step_instruction().unwrap();
                }
                assert_eq!(m.registers().d[0] & 0xff, 0xff);
                assert_eq!(m.registers().d[1] & 0xff, 0x5a);
                assert_eq!(m.registers().d[2] & 0xff, 0xa5);
                let save = m.dump_save_ram().unwrap();
                m.reset();
                assert_eq!(m.peek(PORT), Some(0xa5));
                assert_eq!(m.dump_save_ram(), Some(save));
                assert!(m.check_warm_reset().is_err());
                m.clear_rom_patches();
                assert_eq!(m.peek(PORT), Some(0x11));
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn save_and_sf004_actual_dma_read_source_overlay_through_current_owner() {
    for profile in [Profile::Sf004, Profile::Switched8] {
        for region in REGIONS {
            for concurrent in [false, true] {
                let sf004 = matches!(profile, Profile::Sf004);
                let mut program = Vec::new();
                if sf004 {
                    write_byte(&mut program, 0xf01, 0x60);
                }
                for word in [
                    0x8114,
                    0x8f02,
                    0x9302,
                    0x9400,
                    0x9500,
                    0x9608,
                    if sf004 { 0x9700 } else { 0x9710 },
                    0x4400,
                    0x0080,
                ] {
                    program.extend([0x33fc, word, 0x00c0, 4]);
                }
                program.extend([0x60fe, 0x4e71]);
                let mut m = machine(profile, &program, region, concurrent);
                let source = if sf004 { BANK_SOURCE } else { 0x201000 };
                m.install_rom_patches(&[
                    patch(source, 0xa5),
                    patch(source + 1, 0x5a),
                    patch(source + 2, 0xb6),
                    patch(source + 3, 0x6b),
                ])
                .unwrap();
                for _ in 0..256 {
                    m.step_instruction().unwrap();
                    if m.vdp().memory().vram()[0x400..0x404] == [0xa5, 0x5a, 0xb6, 0x6b] {
                        break;
                    }
                }
                assert_eq!(
                    &m.vdp().memory().vram()[0x400..0x404],
                    &[0xa5, 0x5a, 0xb6, 0x6b]
                );
                assert_eq!(m.sound_fault(), None);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf004_cold_reset_uses_source_zero_vectors_and_alias_prime() {
    for region in REGIONS {
        let mut m = machine(Profile::Sf004, &[0x60fe], region, true);
        select_sf004(&mut m, 6, true);
        m.install_rom_patches(&patches(
            "000004:8010 + 000006:0200 + 000200:7007 + 100200:7009",
        ))
        .unwrap();
        let before = m.clone();
        for code in [
            "000007:01",
            "000004:0020 + 000006:0000",
            "000004:001F + 000006:FFFE",
        ] {
            assert!(m.install_rom_patches(&patches(code)).is_err());
            assert_eq!(m, before);
        }
        let mut end = m.clone();
        end.install_rom_patches(&patches("000004:001F + 000006:FFFC"))
            .unwrap();
        end.reset();
        assert_eq!(end.registers().pc, 0x1ffffc);
        assert_eq!(end.queued_words(), Some([0x1111, 0x1111]));
        m.reset();
        assert_eq!(m.registers().pc, 0x80100200);
        assert_eq!(m.queued_words(), Some([0x7007, 0x00ff]));
        m.step_instruction().unwrap();
        assert_eq!(m.registers().d[0], 7);
        let before = m.clone();
        assert!(m.reset_cpu().is_err());
        assert_eq!(m, before);
        assert!(m.check_warm_reset().is_err());
    }
}
