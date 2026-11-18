use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn large_actual_guest_main_reads_and_queued_origins_use_high_sources() {
    for region in REGIONS {
        for concurrent in [false, true] {
            let mut program = Vec::new();
            write_byte(&mut program, 0xa130f2, 16);
            program.extend([0x1039, 8, 0x1000, 0x1239, 8, 0x1001, 0x3439, 8, 0x1000]);
            write_byte(&mut program, 0xa130f2, 17);
            program.extend([0x3639, 8, 0x1000, 0x60fe, 0x4e71]);
            let mut m = machine(&program, region, concurrent);
            m.install_rom_patches(&patches("ROM:00801000:A55A + ROM:00881000:B66B"))
                .unwrap();
            for _ in 0..6 {
                m.step_instruction().unwrap();
            }
            assert_eq!(m.registers().d[0] & 0xff, 0xa5);
            assert_eq!(m.registers().d[1] & 0xff, 0x5a);
            assert_eq!(m.registers().d[2] & 0xffff, 0xa55a);
            assert_eq!(m.registers().d[3] & 0xffff, 0xb66b);
            let boot = [0x13fc, 16, 0x00a1, 0x30f2, 0x4ef9, 8, 0x1000, 0x4e71];
            let mut m = machine(&boot, region, concurrent);
            m.install_rom_patches(&patches("ROM:00801000:7007 + ROM:00801004:4567"))
                .unwrap();
            m.step_instruction().unwrap();
            m.step_instruction().unwrap();
            assert_eq!(m.queued_words(), Some([0x7007, 0x0640]));
            let inputs = m.cpu.queued_instruction_inputs().unwrap();
            assert_eq!(
                inputs[0].origin.map(|origin| origin.storage_offset()),
                [Some(0x801000), Some(0x801001)]
            );
            select(&mut m, 1, 17);
            m.install_rom_patches(&patches("ROM:00801000:7009"))
                .unwrap();
            assert_eq!(m.peek(0x81000), Some(17));
            assert_eq!(m.cpu.queued_instruction_inputs().unwrap(), inputs);
            m.step_instruction().unwrap();
            assert_eq!(m.registers().d[0], 7);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn large_actual_guest_dma_reads_both_high_banks() {
    for family in [Family::Extended] {
        for region in REGIONS {
            for concurrent in [false, true] {
                for page in [16, 17] {
                    let mut program = Vec::new();
                    write_byte(&mut program, control(family, 1), page);
                    for word in [
                        0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x9608, 0x9704, 0x4400, 0x0080,
                    ] {
                        program.extend([0x33fc, word, 0x00c0, 4]);
                    }
                    program.extend([0x60fe, 0x4e71]);
                    let mut machine = machine(&program, region, concurrent);
                    machine
                        .install_rom_patches(&patches(
                            "ROM:00801000:A55A + ROM:00801002:B66B + ROM:00881000:C77C + ROM:00881002:D88D",
                        ))
                        .unwrap();
                    let expected: [u8; 4] = if page == 16 {
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
