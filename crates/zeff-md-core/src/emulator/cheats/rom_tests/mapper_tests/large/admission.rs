use super::*;
use crate::cartridge::{extended_ssf::ExtendedSsf, realtec::Realtec};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn large_factory_sources_admit_first_boundary_and_reject_custom_owners_atomically() {
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
    use zeff_m68k::timing::ClockedBus;

    for (size, last, page, within, original_byte, hole) in [
        (0x800001, 0x800000, 16, 0, 16, Some(31)),
        (0x880004, 0x880003, 17, 3, 17, Some(31)),
        (0x1000000, 0xffffff, 31, 0x7ffff, 31, None),
        (0x1000001, 0x1000000, 32, 0, 32, Some(63)),
        (32_021_508, 0x1e89c03, 61, 0x9c03, 61, Some(63)),
        (0x2000000, 0x1ffffff, 63, 0x7ffff, 63, None),
        (0x2000001, 0x2000000, 64, 0, 64, Some(127)),
        (0x4000000, 0x3ffffff, 127, 0x7ffff, 127, None),
        (0x4000001, 0x4000000, 128, 0, 128, Some(255)),
        (84 * 1024 * 1024, 0x53fffff, 167, 0x7ffff, 167, Some(255)),
        (0x8000000, 0x7ffffff, 255, 0x7ffff, 255, None),
    ] {
        let mut m =
            Emulator::new(image(size, &[0x60fe, 0x4e71]), config(Region::NtscUs, true)).unwrap();
        assert!(m.supports_rom_cheats());
        let original = m.bus.cartridge.bytes.clone();
        let digest = m.bus.cartridge.rom_digest();
        assert_eq!(original[last as usize], original_byte);
        m.install_rom_patches(&[patch(last, 0xa5)]).unwrap();
        select(&mut m, 1, page);
        let address = 0x80000 + within;
        assert_eq!(m.peek(address), Some(0xa5));
        m.install_rom_patches(&[
            patch(last, 0xa5),
            CheatPatch::WideRomWriteIfEquals {
                address: last,
                value: CheatValue::Constant(0x5a),
                compare: CheatValue::Constant(original_byte),
            },
            CheatPatch::WideRomWriteIfEquals {
                address: last,
                value: CheatValue::Constant(0xee),
                compare: CheatValue::Constant(0xa5),
            },
        ])
        .unwrap();
        assert_eq!(m.peek(address), Some(0x5a));
        assert_eq!(
            m.bus.cartridge.rom_source_offset(address),
            Some(last as usize)
        );
        let upper = address & 1 == 0;
        let start = m.cpu.clock();
        let input = ScheduledBus::new(&mut m.bus, &mut m.sound_cpu, &mut m.sound_schedule)
            .read_instruction_cycle(
                BusAccess {
                    logical_address: address,
                    bus_address: address & !1,
                    lanes: if upper {
                        ByteLanes::Upper
                    } else {
                        ByteLanes::Lower
                    },
                    kind: AccessKind::Data,
                    function_code: FunctionCode::SupervisorData,
                },
                start,
            )
            .result
            .unwrap();
        assert_eq!(input.value, if upper { 0x5a00 } else { 0x005a });
        assert_eq!(
            input.origin[usize::from(!upper)].storage_offset(),
            Some(u64::from(last))
        );
        assert_eq!(m.sound_fault(), None);
        if let Some(hole) = hole {
            if within != 0x7ffff {
                assert_eq!(m.peek(address + 1), None);
            }
            select(&mut m, 1, hole);
            assert_eq!(m.peek(0x80000), None);
            assert_eq!(m.bus.cartridge.rom_source_offset(0x80000), None);
        } else {
            select(&mut m, 1, 255);
            assert_eq!(m.peek(0xfffff), Some(0x5a));
            assert_eq!(
                m.bus.cartridge.rom_source_offset(0xfffff),
                Some(last as usize)
            );
        }
        select(&mut m, 1, page);
        let before = m.clone();
        for bad in [size as u32, 0x8000000, u32::MAX] {
            assert!(
                m.install_rom_patches(&[patch(last, 0), patch(bad, 1)])
                    .is_err()
            );
            assert_eq!(m, before);
        }
        assert!(Arc::ptr_eq(&original, &m.bus.cartridge.bytes));
        assert_eq!(m.bus.cartridge.rom_digest(), digest);
        m.clear_rom_patches();
        assert_eq!(m.peek(address), Some(original_byte));
        assert_eq!(original[last as usize], original_byte);
        assert!(
            Emulator::new_with_timing(
                image(size, &[0x60fe, 0x4e71]),
                config(Region::NtscUs, true),
                TimingModel::Precise,
            )
            .is_err()
        );
    }
    let source = machine(&[0x60fe, 0x4e71], Region::NtscUs, true);
    for defect in 0..7 {
        let mut m = source.clone();
        match defect {
            0 => m.bus.cartridge.extended_ssf = None,
            1 => m.bus.cartridge.extended_ssf = Some(ExtendedSsf::new(0x1000000)),
            2 => m.bus.cartridge.bytes = m.bus.cartridge.bytes.to_vec().into(),
            3 => Arc::make_mut(&mut m.bus.cartridge.bytes)[0x801000] ^= 1,
            4 => m.bus.cartridge.realtec = Some(Realtec::new()),
            5 => m.bus.cartridge.ssf2 = Some(crate::cartridge::ssf2::Ssf2::new()),
            6 => {
                m.bus.cartridge.save =
                    Some(crate::cartridge::ram::profile_tests::origin::sf002_save())
            }
            _ => unreachable!(),
        }
        assert!(!m.supports_rom_cheats(), "defect {defect}");
        let before = m.clone();
        assert!(m.install_rom_patches(&[patch(0x801000, 0xa5)]).is_err());
        assert_eq!(m, before);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn large_slots_alias_original_compares_partial_pages_and_cold_reset() {
    let mut m = machine(&[0x60fe, 0x4e71], Region::NtscUs, true);
    let mut set =
        patches("ROM:00801000:A55A + ROM:0087FFFF:C7 + ROM:00880000:D8 + ROM:00900003:E9");
    set.extend([
        CheatPatch::WideRomWriteIfEquals {
            address: 0x801001,
            value: CheatValue::Constant(0x66),
            compare: CheatValue::Constant(1),
        },
        CheatPatch::WideRomWriteIfEquals {
            address: 0x801001,
            value: CheatValue::Constant(0xee),
            compare: CheatValue::Constant(0x5a),
        },
    ]);
    m.install_rom_patches(&set).unwrap();
    for slot in 0..8 {
        select(&mut m, slot, 16);
        assert_eq!(m.peek(slot * 0x80000 + 0x1000), Some(0xa5));
        assert_eq!(m.peek(slot * 0x80000 + 0x1001), Some(0x66));
        assert_eq!(m.peek(slot * 0x80000 + 0x7ffff), Some(0xc7));
        select(&mut m, slot, 17);
        assert_eq!(m.peek(slot * 0x80000), Some(0xd8));
        select(&mut m, slot, 18);
        assert_eq!(m.peek(slot * 0x80000 + 3), Some(0xe9));
        assert_eq!(m.peek(slot * 0x80000 + 4), None);
        select(&mut m, slot, 31);
        assert_eq!(m.peek(slot * 0x80000), None);
    }
    m.install_rom_patches(&patches("000200:7007 + ROM:00801000:A5"))
        .unwrap();
    m.reset();
    assert_eq!(m.queued_words(), Some([0x7007, 0x4e71]));
    assert_eq!(m.peek(0x81000), Some(1));
    m.step_instruction().unwrap();
    assert_eq!(m.registers().d[0], 7);
    assert!(m.check_warm_reset().is_err());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn large_final_page_keeps_full_source_offset_and_byte_word_limits() {
    let mut m = Emulator::new(
        image(0x8000000, &[0x60fe, 0x4e71]),
        config(Region::NtscUs, true),
    )
    .unwrap();
    assert!(m.supports_rom_cheats());
    m.install_rom_patches(&patches("ROM:07FFFFFE:A55A"))
        .unwrap();
    for slot in 0..8 {
        select(&mut m, slot, 255);
        assert_eq!(m.peek(slot * 0x80000 + 0x7fffe), Some(0xa5));
        assert_eq!(m.peek(slot * 0x80000 + 0x7ffff), Some(0x5a));
        let sampled = m
            .bus
            .cartridge
            .read_sampled(slot * 0x80000 + 0x7ffff)
            .unwrap();
        assert_eq!(sampled.origin.storage_offset(), Some(0x7ffffff));
    }
    m.clear_rom_patches();
    assert_eq!(m.peek(0x3fffff), Some(255));
}
