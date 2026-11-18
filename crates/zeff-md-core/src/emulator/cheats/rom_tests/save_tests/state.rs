use super::*;
use zeff_emu_common::save_state::StateReader;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn save_and_sf004_v1_restore_keeps_prefetch_cells_and_receiver_overlay() {
    for profile in [Profile::Sf004, Profile::Switched8, Profile::Split256] {
        for region in REGIONS {
            let sf004 = matches!(profile, Profile::Sf004);
            let mut boot = Vec::new();
            if sf004 {
                write_byte(&mut boot, 0xf01, 0x60);
            }
            boot.extend([0x4ef9, 0, 0x1000]);
            let mut m = machine(profile, &boot, region, true);
            let offset = if sf004 { BANK_SOURCE } else { 0x1000 };
            m.install_rom_patches(&[patch(offset + 1, 7)]).unwrap();
            let cells = vec![0x5a; m.dump_save_ram().unwrap().len()];
            m.load_save_ram(&cells).unwrap();
            for _ in 0..if sf004 { 2 } else { 1 } {
                m.step_instruction().unwrap();
            }
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
                            .install_rom_patches(&[patch(offset + 1, value)])
                            .unwrap();
                    }
                    receiver.load_save_ram(&vec![0x33; cells.len()]).unwrap();
                    if sf004 {
                        select_sf004(&mut receiver, 7, true);
                    }
                    let overlay = receiver.bus.cartridge.rom_overlay.clone();
                    restore(&mut receiver, &raw, &front, frontend);
                    assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
                    assert_eq!(receiver.dump_save_ram(), Some(cells.clone()));
                    assert_eq!(receiver.peek(0x1001), Some(current.unwrap_or(1)));
                    assert_eq!(receiver.queued_words(), Some([0x7007, 0x0640]));
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], 7);
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], 10);
                    receiver.step_instruction().unwrap();
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], u32::from(current.unwrap_or(1)));
                }
            }
        }
    }
}

fn section_start(raw: &[u8], after: usize) -> usize {
    let mut reader = StateReader::new(raw);
    reader.set_position(52);
    for _ in 0..3 {
        reader
            .read_slice(crate::save_state::MAX_STATE_BYTES)
            .unwrap();
    }
    reader.read_bool().unwrap();
    for _ in 0..after {
        reader
            .read_slice(crate::save_state::MAX_STATE_BYTES)
            .unwrap();
    }
    reader.position() + 4
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn save_and_sf004_bad_owner_state_hash_and_truncation_preserve_overlay() {
    for profile in [Profile::Sf004, Profile::Switched8, Profile::Split256] {
        let mut m = machine(profile, &[0x60fe], Region::NtscUs, true);
        m.install_rom_patches(&[patch(0x1000, 0xa5)]).unwrap();
        let raw = m.encode_state().unwrap();
        let front = m.encode_frontend_state().unwrap();
        let mut bad_owner = raw.clone();
        if matches!(profile, Profile::Sf004) {
            let at = section_start(&raw, 5);
            assert_eq!(&raw[at..at + 5], &[1, b'S', b'F', b'0', b'4']);
            bad_owner[at + 5] = 8;
        } else {
            let at = section_start(&raw, 0);
            assert_eq!(raw[at], 1);
            bad_owner[at] = 0;
        }
        let mut bad_hash = raw.clone();
        bad_hash[12] ^= 1;
        let mut trailing = raw.clone();
        trailing.push(0);
        let before = m.clone();
        for bad in [bad_owner, bad_hash, trailing, raw[..raw.len() - 1].to_vec()] {
            assert!(m.decode_state(&bad).is_err());
            assert_eq!(m, before);
        }
        for bad in [&front[..front.len() - 1], &front[..front.len() / 2]] {
            assert!(m.decode_frontend_state(bad).is_err());
            assert_eq!(m, before);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn save_and_sf004_dma_fifo_keeps_sample_and_refreshes_unsampled_source() {
    use crate::hardware::vdp::VdpDmaSource;

    for profile in [Profile::Sf004, Profile::Switched8] {
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
        ] {
            program.extend([0x33fc, word, 0x00c0, 4]);
        }
        program.extend([0x60fe, 0x4e71]);
        let mut m = machine(profile, &program, Region::NtscUs, true);
        let source = if sf004 { BANK_SOURCE } else { 0x201000 };
        let unpatched: [u8; 2] = m.rom_bytes()[source as usize + 2..source as usize + 4]
            .try_into()
            .unwrap();
        m.install_rom_patches(&[
            patch(source, 0xa5),
            patch(source + 1, 0x5a),
            patch(source + 2, 0xb6),
            patch(source + 3, 0x6b),
        ])
        .unwrap();
        for _ in 0..if sf004 { 9 } else { 8 } {
            m.step_instruction().unwrap();
        }
        m.bus
            .vdp
            .write_control_from(
                0x0080,
                Region::NtscUs,
                VdpDmaSource::from_cartridge(&m.bus.cartridge, &m.bus.ram),
            )
            .unwrap();
        for _ in 0..2000 {
            if m.vdp()
                .fifo()
                .front()
                .is_some_and(|entry| entry.data == 0xa55a)
            {
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
                        .install_rom_patches(&[patch(source + 2, value)])
                        .unwrap();
                }
                restore(&mut receiver, &raw, &front, frontend);
                assert_eq!(receiver.vdp().fifo().front(), Some(first));
                advance(&mut receiver, 2000);
                assert_eq!(&receiver.vdp().memory().vram()[0x400..0x402], &[0xa5, 0x5a]);
                assert_eq!(
                    &receiver.vdp().memory().vram()[0x402..0x404],
                    &[current.unwrap_or(unpatched[0]), unpatched[1]]
                );
            }
        }
    }
}
