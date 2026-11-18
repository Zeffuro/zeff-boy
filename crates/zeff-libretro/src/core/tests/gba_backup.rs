use super::*;

#[test]
fn gba_libretro_reset_preserves_backup_and_rtc() {
    let mut state =
        CoreState::from_rom(&emerald_flash1m_rom(), "emerald.gba").expect("GBA ROM should load");
    let date_time =
        zeff_gba_core::hardware::cartridge::RtcDateTime::new(2024, 2, 29, 4, [12, 34, 56])
            .expect("valid RTC date/time");
    let mut backup = vec![0xFF; state.sram_size()];
    backup[0x1234] = 0x5A;

    {
        let ActiveCore::Gba(emu) = &mut state.core else {
            panic!("expected GBA core");
        };
        emu.load_battery_sram(&backup)
            .expect("Flash save should load");
        assert!(emu.set_rtc_date_time(date_time));
    }

    state.reset().unwrap();

    let ActiveCore::Gba(emu) = &state.core else {
        panic!("expected GBA core");
    };
    assert_eq!(emu.dump_battery_sram(), Some(backup));
    assert_eq!(emu.rtc_date_time(), Some(date_time));
}

#[test]
fn gba_sync_sram_to_buf_matches_owned_backup_for_all_kinds() {
    let cases: &[(&str, &[u8])] = &[
        ("none.gba", b""),
        ("sram.gba", b"SRAM_V113"),
        ("flash512.gba", b"FLASH512_V131"),
        ("flash1m.gba", b"FLASH1M_V103"),
        ("eeprom.gba", b"EEPROM_V122"),
    ];

    for &(path, marker) in cases {
        let mut state =
            CoreState::from_rom(&gba_backup_rom(marker), path).expect("GBA ROM should load");
        let expected = state.battery_sram();
        let mut output = expected
            .as_ref()
            .map_or_else(Vec::new, |bytes| vec![0xA5; bytes.len()]);
        if let Some(expected) = &expected {
            let mut data = expected.clone();
            for (index, byte) in data.iter_mut().enumerate() {
                *byte = index as u8;
            }
            state.load_battery_sram(&data).unwrap();
        }

        let expected = state.battery_sram();
        state.sync_sram_to_buf(&mut output).unwrap();
        match expected {
            Some(expected) => assert_eq!(output, expected),
            None => assert!(output.is_empty()),
        }
    }
}
