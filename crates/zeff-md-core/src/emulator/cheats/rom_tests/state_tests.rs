use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn queued_rom_samples_retire_before_new_reads_observe_replacement_or_removal() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            for next in [Some(9), None] {
                let mut machine = fixture(&[0x4ef9, 1, 0x0100, 0x4e71], timing, region);
                machine.install_rom_patches(&[patch(CODE + 1, 7)]).unwrap();
                machine.step_instruction().unwrap();
                assert_eq!(machine.queued_words(), Some([0x7007, 0x0640]));
                let before = machine.encode_frontend_state().unwrap();
                if let Some(value) = next {
                    machine
                        .install_rom_patches(&[patch(CODE + 1, value)])
                        .unwrap();
                } else {
                    machine.clear_rom_patches();
                }
                assert_eq!(machine.encode_frontend_state().unwrap(), before);
                machine.step_instruction().unwrap();
                assert_eq!(machine.registers().d[0], 7);
                machine.step_instruction().unwrap();
                assert_eq!(machine.registers().d[0], 10);
                machine.step_instruction().unwrap();
                machine.step_instruction().unwrap();
                assert_eq!(machine.registers().d[0], u32::from(next.unwrap_or(1)));
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn version_one_restore_keeps_saved_samples_and_the_receivers_external_rom_set() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut source = fixture(&[0x4ef9, 1, 0x0100, 0x4e71], timing, region);
            let set_a = [patch(CODE + 1, 7), patch(DATA, 0xa5)];
            source.install_rom_patches(&set_a).unwrap();
            source.step_instruction().unwrap();
            assert_eq!(source.queued_words(), Some([0x7007, 0x0640]));
            let raw = source.encode_state().unwrap();
            let frontend = source.encode_frontend_state().unwrap();
            assert_eq!(&raw[..8], b"ZMDRAW01");
            assert_eq!(&frontend[..8], b"ZMDFRONT");
            assert_eq!(&raw[8..12], &1_u32.to_le_bytes());
            assert_eq!(&frontend[8..12], &1_u32.to_le_bytes());
            for current in [Some((7, 0xa5)), Some((9, 0x5a)), None] {
                for front in [false, true] {
                    let mut receiver = fixture(&[0x4ef9, 1, 0x0100, 0x4e71], timing, region);
                    if let Some((opcode, data)) = current {
                        receiver
                            .install_rom_patches(&[patch(CODE + 1, opcode), patch(DATA, data)])
                            .unwrap();
                    }
                    let overlay = receiver.bus.cartridge.rom_overlay.clone();
                    if front {
                        receiver.decode_frontend_state(&frontend).unwrap();
                        assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
                    } else {
                        receiver.decode_state(&raw).unwrap();
                        assert_eq!(receiver.encode_state().unwrap(), raw);
                    }
                    assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
                    assert_eq!(receiver.queued_words(), Some([0x7007, 0x0640]));
                    assert_eq!(
                        receiver.peek(DATA),
                        Some(current.map_or(0x12, |(_, data)| data))
                    );
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], 7);
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], 10);
                    receiver.step_instruction().unwrap();
                    receiver.step_instruction().unwrap();
                    assert_eq!(
                        receiver.registers().d[0],
                        u32::from(current.map_or(1, |(opcode, _)| opcode))
                    );
                }
            }
            let mut receiver = source.clone();
            receiver.clear_rom_patches();
            receiver.decode_frontend_state(&frontend).unwrap();
            receiver.install_rom_patches(&set_a).unwrap();
            receiver.install_rom_patches(&set_a).unwrap();
            assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
            for _ in 0..8 {
                assert_eq!(source.step_instruction(), receiver.step_instruction());
                assert_eq!(
                    source.encode_frontend_state().unwrap(),
                    receiver.encode_frontend_state().unwrap()
                );
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn raw_and_frontend_failed_decode_preserve_live_machine_and_overlay() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut machine = fixture(&[0x60fe, 0x4e71], timing, Region::NtscUs);
        machine.install_rom_patches(&[patch(DATA, 0xa5)]).unwrap();
        machine.step_instruction().unwrap();
        let raw = machine.encode_state().unwrap();
        let front = machine.encode_frontend_state().unwrap();
        let mut bad_raw_magic = raw.clone();
        bad_raw_magic[0] ^= 1;
        let mut bad_raw_hash = raw.clone();
        bad_raw_hash[12] ^= 1;
        let mut trailing_raw = raw.clone();
        trailing_raw.push(0);
        let mut bad_front_magic = front.clone();
        bad_front_magic[0] ^= 1;
        let mut bad_front_core = front.clone();
        bad_front_core[28] ^= 1;
        let mut trailing_front = front.clone();
        trailing_front.push(0);
        let before = machine.clone();
        for bytes in [
            &raw[..raw.len() - 1],
            &bad_raw_magic,
            &bad_raw_hash,
            &trailing_raw,
        ] {
            assert!(machine.decode_state(bytes).is_err());
            assert_eq!(machine, before);
        }
        for bytes in [
            &front[..front.len() - 1],
            &bad_front_magic,
            &bad_front_core,
            &trailing_front,
        ] {
            assert!(machine.decode_frontend_state(bytes).is_err());
            assert_eq!(machine, before);
        }
        let mut other_rom = image(&[0x60fe, 0x4e71]);
        other_rom[DATA as usize] ^= 1;
        let mut other = Emulator::new_with_timing(
            other_rom,
            LoadConfig {
                region: Region::NtscUs,
                concurrent_sound: true,
            },
            timing,
        )
        .unwrap();
        other.install_rom_patches(&[patch(DATA, 0x5a)]).unwrap();
        let before = other.clone();
        assert!(other.decode_state(&raw).is_err());
        assert!(other.decode_frontend_state(&front).is_err());
        assert_eq!(other, before);
    }
}
