use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapper_guest_controls_main_byte_word_opcode_and_operand_reads() {
    for family in FAMILIES {
        for region in REGIONS {
            for concurrent in [false, true] {
                let mut program = Vec::new();
                write_byte(&mut program, control(family, 1), 8);
                program.extend([0x1039, 8, 0x1000, 0x1239, 8, 0x1001, 0x3439, 8, 0x1000]);
                write_byte(&mut program, control(family, 1), 9);
                program.extend([0x3639, 8, 0x1000, 0x60fe, 0x4e71]);
                let mut machine = mapper(family, &program, region, concurrent);
                machine
                    .install_rom_patches(&patches("401000:A55A + 481000:B66B"))
                    .unwrap();
                for _ in 0..6 {
                    machine.step_instruction().unwrap();
                }
                assert_eq!(machine.registers().d[0] & 0xff, 0xa5);
                assert_eq!(machine.registers().d[1] & 0xff, 0x5a);
                assert_eq!(machine.registers().d[2] & 0xffff, 0xa55a);
                assert_eq!(machine.registers().d[3] & 0xffff, 0xb66b);
                assert_eq!(machine.peek(WINDOW), Some(0xb6));
                assert_eq!(
                    machine.bus.cartridge.rom_source_offset(WINDOW),
                    Some(source(9, WITHIN) as usize)
                );

                let mut boot = Vec::new();
                write_byte(&mut boot, control(family, 1), 8);
                boot.extend([0x4ef9, 8, 0x1000, 0x4e71]);
                let mut code = vec![8; WITHIN as usize + 12];
                write_words(
                    &mut code,
                    WITHIN as usize,
                    &[0x7001, 0x0640, 3, 0x4ef9, 8, 0x1000],
                );
                let mut execution = mapper(family, &boot, region, concurrent);
                replace_page(&mut execution, family, 8, &code);
                execution
                    .install_rom_patches(&patches("401000:7007 + 401004:4567"))
                    .unwrap();
                execution.step_instruction().unwrap();
                execution.step_instruction().unwrap();
                assert_eq!(execution.queued_words(), Some([0x7007, 0x0640]));
                execution.step_instruction().unwrap();
                execution.step_instruction().unwrap();
                assert_eq!(execution.registers().d[0], 0x456e);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapper_actual_guest_dma_reads_patched_high_banks() {
    for family in FAMILIES {
        for region in REGIONS {
            for concurrent in [false, true] {
                for page in [8, 9] {
                    let mut program = Vec::new();
                    write_byte(&mut program, control(family, 1), page);
                    for word in [
                        0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x9608, 0x9704, 0x4400, 0x0080,
                    ] {
                        program.extend([0x33fc, word, 0x00c0, 4]);
                    }
                    program.extend([0x60fe, 0x4e71]);
                    let mut machine = mapper(family, &program, region, concurrent);
                    machine
                        .install_rom_patches(&patches(
                            "401000:A55A + 401002:B66B + 481000:C77C + 481002:D88D",
                        ))
                        .unwrap();
                    let expected: [u8; 4] = if page == 8 {
                        [0xa5, 0x5a, 0xb6, 0x6b]
                    } else {
                        [0xc7, 0x7c, 0xd8, 0x8d]
                    };
                    for _ in 0..256 {
                        machine.step_instruction().unwrap();
                        if machine.vdp().memory().vram()[0x400..0x404] == expected {
                            break;
                        }
                    }
                    assert_eq!(&machine.vdp().memory().vram()[0x400..0x404], &expected);
                    assert_eq!(machine.sound_fault(), None);
                }
            }
        }
    }
}
