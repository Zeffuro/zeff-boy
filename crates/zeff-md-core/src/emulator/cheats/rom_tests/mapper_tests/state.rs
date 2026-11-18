use super::*;
use zeff_emu_common::save_state::StateReader;

fn code_fixture(family: Family, region: Region) -> Emulator {
    let mut boot = Vec::new();
    write_byte(&mut boot, control(family, 1), 8);
    boot.extend([0x4ef9, 8, 0x1000, 0x4e71]);
    let mut code = vec![8; WITHIN as usize + 12];
    write_words(
        &mut code,
        WITHIN as usize,
        &[0x7001, 0x0640, 3, 0x4ef9, 8, 0x1000],
    );
    let mut machine = mapper(family, &boot, region, true);
    replace_page(&mut machine, family, 8, &code);
    machine
}

pub(super) fn restore(receiver: &mut Emulator, raw: &[u8], frontend: &[u8], front: bool) {
    if front {
        receiver.decode_frontend_state(frontend).unwrap();
        assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
    } else {
        receiver.decode_state(raw).unwrap();
        assert_eq!(receiver.encode_state().unwrap(), raw);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapper_v1_restore_keeps_prefetch_receiver_overlay_and_restores_banks() {
    for family in FAMILIES {
        for region in REGIONS {
            let mut source_machine = code_fixture(family, region);
            source_machine
                .install_rom_patches(&[patch(source(8, WITHIN + 1), 7)])
                .unwrap();
            source_machine.step_instruction().unwrap();
            source_machine.step_instruction().unwrap();
            assert_eq!(source_machine.queued_words(), Some([0x7007, 0x0640]));
            let raw = source_machine.encode_state().unwrap();
            let front = source_machine.encode_frontend_state().unwrap();
            assert_eq!(&raw[8..12], &1_u32.to_le_bytes());
            assert_eq!(&front[8..12], &1_u32.to_le_bytes());
            for current in [Some(7), Some(9), None] {
                for frontend in [false, true] {
                    let mut receiver = source_machine.clone();
                    receiver.clear_rom_patches();
                    if let Some(value) = current {
                        receiver
                            .install_rom_patches(&[patch(source(8, WITHIN + 1), value)])
                            .unwrap();
                    }
                    select(&mut receiver, family, 1, 9);
                    let overlay = receiver.bus.cartridge.rom_overlay.clone();
                    restore(&mut receiver, &raw, &front, frontend);
                    assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
                    assert_eq!(receiver.peek(WINDOW + 1), Some(current.unwrap_or(1)));
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

fn mapper_page_offset(raw: &[u8], family: Family) -> usize {
    if matches!(family, Family::Ssf2) {
        return crate::emulator::state::tests::layout::capabilities_offset(raw) + 3;
    }
    let mut reader = StateReader::new(raw);
    reader.set_position(52);
    for _ in 0..3 {
        reader
            .read_slice(crate::save_state::MAX_STATE_BYTES)
            .unwrap();
    }
    reader.read_bool().unwrap();
    for _ in 0..6 {
        reader
            .read_slice(crate::save_state::MAX_STATE_BYTES)
            .unwrap();
    }
    let at = reader.position() + 4;
    let section = reader
        .read_slice(crate::save_state::MAX_STATE_BYTES)
        .unwrap();
    assert_eq!(&section[..5], &[1, b'E', b'S', b'F', b'1']);
    at + 5
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapper_bad_page_hash_and_truncated_state_preserve_live_overlay() {
    for family in FAMILIES {
        let mut machine = mapper(family, &[0x60fe, 0x4e71], Region::NtscUs, true);
        select(&mut machine, family, 1, 8);
        machine
            .install_rom_patches(&patches("401000:A55A"))
            .unwrap();
        let raw = machine.encode_state().unwrap();
        let front = machine.encode_frontend_state().unwrap();
        let mut bad_page = raw.clone();
        bad_page[mapper_page_offset(&raw, family)] = 0x80;
        let mut bad_hash = raw.clone();
        bad_hash[12] ^= 1;
        let mut trailing = raw.clone();
        trailing.push(0);
        let before = machine.clone();
        for bad in [bad_page, bad_hash, trailing, raw[..raw.len() - 1].to_vec()] {
            assert!(machine.decode_state(&bad).is_err());
            assert_eq!(machine, before);
        }
        for bad in [&front[..front.len() - 1], &front[..front.len() / 2]] {
            assert!(machine.decode_frontend_state(bad).is_err());
            assert_eq!(machine, before);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapper_dma_fifo_preserves_sample_and_future_fetch_uses_receiver_overlay() {
    use crate::hardware::vdp::VdpDmaSource;

    for family in FAMILIES {
        for region in REGIONS {
            let mut program = Vec::new();
            write_byte(&mut program, control(family, 1), 8);
            for word in [
                0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x9608, 0x9704, 0x4400,
            ] {
                program.extend([0x33fc, word, 0x00c0, 4]);
            }
            program.extend([0x60fe, 0x4e71]);
            let mut source_machine = mapper(family, &program, region, true);
            source_machine
                .install_rom_patches(&patches("401000:A55A + 401002:B66B"))
                .unwrap();
            for _ in 0..9 {
                source_machine.step_instruction().unwrap();
            }
            source_machine
                .bus
                .vdp
                .write_control_from(
                    0x0080,
                    region,
                    VdpDmaSource::from_cartridge(
                        &source_machine.bus.cartridge,
                        &source_machine.bus.ram,
                    ),
                )
                .unwrap();
            for _ in 0..2000 {
                if source_machine
                    .vdp()
                    .fifo()
                    .front()
                    .is_some_and(|entry| entry.data == 0xa55a)
                {
                    break;
                }
                advance(&mut source_machine, 1);
            }
            let first = source_machine
                .vdp()
                .fifo()
                .front()
                .expect("sampled mapper DMA word");
            assert_eq!(first.data, 0xa55a);
            assert_ne!(
                &source_machine.vdp().memory().vram()[0x400..0x402],
                &[0xa5, 0x5a]
            );
            let raw = source_machine.encode_state().unwrap();
            let front = source_machine.encode_frontend_state().unwrap();
            for (set, expected) in [
                ("401000:A55A + 401002:B66B", [0xb6, 0x6b]),
                ("401000:C77C + 401002:D88D", [0xd8, 0x8d]),
                ("", [8, 8]),
            ] {
                for frontend in [false, true] {
                    let mut receiver = source_machine.clone();
                    if set.is_empty() {
                        receiver.clear_rom_patches();
                    } else {
                        receiver.install_rom_patches(&patches(set)).unwrap();
                    }
                    restore(&mut receiver, &raw, &front, frontend);
                    assert_eq!(receiver.vdp().fifo().front(), Some(first));
                    advance(&mut receiver, 2000);
                    assert_eq!(&receiver.vdp().memory().vram()[0x400..0x402], &[0xa5, 0x5a]);
                    assert_eq!(&receiver.vdp().memory().vram()[0x402..0x404], &expected);
                }
            }
        }
    }
}
