use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_overlay_v1_restores_map_save_and_prefetch_but_keeps_receiver_configuration() {
    for revision in REVISIONS {
        for region in REGIONS {
            let mut m = with_program(revision, region, true, &[0x4ef9, 0, 0x1000, 0x4e71]);
            control(&mut m, 0xa0);
            m.load_save_ram(&vec![0x5a; 0x10000]).unwrap();
            m.install_rom_patches(&patches("381001:07")).unwrap();
            m.step_instruction().unwrap();
            assert_eq!(m.queued_words(), Some([0x7007, 0x0640]));
            let raw = m.encode_state().unwrap();
            let front = m.encode_frontend_state().unwrap();
            assert_eq!(&raw[8..12], &1_u32.to_le_bytes());
            assert_eq!(&front[8..12], &1_u32.to_le_bytes());
            for current in [Some(7), Some(9), None] {
                for frontend in [false, true] {
                    let mut receiver = m.clone();
                    receiver.clear_rom_patches();
                    if let Some(value) = current {
                        receiver
                            .install_rom_patches(&[patch(0x381001, value)])
                            .unwrap();
                    }
                    receiver.bus.cartridge.sf001.as_mut().unwrap().power_on();
                    receiver.load_save_ram(&vec![0x33; 0x10000]).unwrap();
                    restore(&mut receiver, &raw, &front, frontend);
                    assert_eq!(
                        receiver.bus.cartridge.sf001.as_ref().unwrap().mapping(),
                        Mapping::Switched
                    );
                    assert!(receiver.bus.cartridge.sf001.as_ref().unwrap().locked());
                    assert_eq!(receiver.dump_save_ram(), Some(vec![0x5a; 0x10000]));
                    assert_eq!(receiver.peek(0x1001), Some(current.unwrap_or(1)));
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], 7);
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], 10);
                    receiver.step_instruction().unwrap();
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], u32::from(current.unwrap_or(1)));
                    let before = receiver.clone();
                    let mut bad = raw.clone();
                    bad[12] ^= 1;
                    for bytes in [
                        bad,
                        raw[..raw.len() - 1].to_vec(),
                        [raw.clone(), vec![0]].concat(),
                    ] {
                        assert!(receiver.decode_state(&bytes).is_err());
                        assert_eq!(receiver, before);
                    }
                    assert!(
                        receiver
                            .decode_frontend_state(&front[..front.len() - 1])
                            .is_err()
                    );
                    assert_eq!(receiver, before);
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_overlay_v1_dma_fifo_is_historical_and_unsampled_word_uses_receiver_source() {
    for revision in REVISIONS {
        let mut program = Vec::new();
        for word in [
            0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x9608, 0x9700, 0x4400,
        ] {
            program.extend([0x33fc, word, 0xc0, 4]);
        }
        program.extend([0x60fe, 0x4e71]);
        let mut m = with_program(revision, Region::NtscUs, true, &program);
        control(&mut m, 0x80);
        m.install_rom_patches(&patches("381000:A55A+381002:B66B"))
            .unwrap();
        for _ in 0..8 {
            m.step_instruction().unwrap();
        }
        m.bus
            .vdp
            .write_control_from(
                0x80,
                Region::NtscUs,
                VdpDmaSource::from_cartridge(&m.bus.cartridge, &m.bus.ram),
            )
            .unwrap();
        for _ in 0..2000 {
            if m.vdp().fifo().front().is_some_and(|e| e.data == 0xa55a) {
                break;
            }
            advance(&mut m, 1);
        }
        let first = m.vdp().fifo().front().expect("sampled DMA word");
        assert_eq!(first.data, 0xa55a);
        let raw = m.encode_state().unwrap();
        let front = m.encode_frontend_state().unwrap();
        for current in [Some(0xc7), Some(0xb6), None] {
            for frontend in [false, true] {
                let mut receiver = m.clone();
                receiver.clear_rom_patches();
                if let Some(value) = current {
                    receiver
                        .install_rom_patches(&[patch(0x381002, value)])
                        .unwrap();
                }
                restore(&mut receiver, &raw, &front, frontend);
                assert_eq!(receiver.vdp().fifo().front(), Some(first));
                advance(&mut receiver, 2000);
                assert_eq!(
                    &receiver.vdp().memory().vram()[0x400..0x404],
                    &[0xa5, 0x5a, current.unwrap_or(0x06), 0x40]
                );
            }
        }
    }
}
