use super::*;

#[test]
fn backend_link_peer_sync_exchanges_game_boy_bytes() {
    let mut left = build_gb_backend();
    let mut right = build_gb_backend();

    assert!(left.sync_link_peer(&mut right));

    {
        let (EmuBackend::Gb(left), EmuBackend::Gb(right)) = (&mut left, &mut right) else {
            panic!("expected GB backends");
        };
        left.emu.write_byte(SERIAL_SB, 0xAB);
        right.emu.write_byte(SERIAL_SB, 0x34);
        left.emu.write_byte(SERIAL_SC, 0x81);
        right.emu.write_byte(SERIAL_SC, 0x80);
    }

    left.step_frame();
    right.step_frame();

    assert!(left.sync_link_peer(&mut right));

    let (EmuBackend::Gb(left_gb), EmuBackend::Gb(right_gb)) = (&left, &right) else {
        panic!("expected GB backends");
    };
    assert_eq!(left_gb.emu.cpu_peek8(SERIAL_SB), 0x34);
    assert_eq!(right_gb.emu.cpu_peek8(SERIAL_SB), 0x34);
    assert_ne!(right_gb.emu.cpu_peek8(SERIAL_SC) & 0x80, 0);

    right.step_frame();

    let (EmuBackend::Gb(left), EmuBackend::Gb(right)) = (&left, &right) else {
        panic!("expected GB backends");
    };
    assert_eq!(left.emu.cpu_peek8(SERIAL_SB), 0x34);
    assert_eq!(right.emu.cpu_peek8(SERIAL_SB), 0xAB);
    assert_eq!(left.emu.cpu_peek8(SERIAL_SC) & 0x80, 0);
    assert_eq!(right.emu.cpu_peek8(SERIAL_SC) & 0x80, 0);
    assert_eq!(left.emu.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
    assert_eq!(right.emu.cpu_peek8(INTERRUPT_IF) & 0x08, 0x08);
}

#[test]
fn backend_link_peer_sync_exchanges_wonder_swan_uart_bytes() {
    let mut left = build_ws_backend();
    let mut right = build_ws_backend();

    assert!(left.sync_link_peer(&mut right));

    {
        let (EmuBackend::Ws(left), EmuBackend::Ws(right)) = (&mut left, &mut right) else {
            panic!("expected WonderSwan backends");
        };
        left.emu.io_write8(0x00B3, 0x80);
        right.emu.io_write8(0x00B3, 0x80);
        left.emu.io_write8(0x00B1, 0x5A);
    }

    for _ in 0..64 {
        left.step_frame();
        assert!(left.sync_link_peer(&mut right));
        let EmuBackend::Ws(right) = &right else {
            panic!("expected WonderSwan backend");
        };
        if right.emu.io_peek8(0x00B3) & 0x01 != 0 {
            break;
        }
    }

    let EmuBackend::Ws(right) = &right else {
        panic!("expected WonderSwan backend");
    };
    assert_eq!(right.emu.io_peek8(0x00B3) & 0x01, 0x01);
    assert_eq!(right.emu.io_peek8(0x00B1), 0x5A);
}

#[test]
fn backend_link_peer_sync_rejects_incompatible_pairs() {
    let mut gb = build_gb_backend();
    let mut gba = build_gba_backend();

    assert!(!gb.sync_link_peer(&mut gba));
}

#[test]
fn sega8_link_sync_and_detached_factory_matrix_is_explicit() {
    let backend = |hint, name| {
        let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(&[0x00], 44_100, hint)
            .expect("Sega8 matrix fixture should initialize");
        EmuBackend::from_sega8(emu, PathBuf::from(name))
    };
    let mut sms_left = backend(
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
        "left.sms",
    );
    let mut sms_right = backend(
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
        "right.sms",
    );
    assert!(!sms_left.sync_link_peer(&mut sms_right));
    for sms in [&sms_left, &sms_right] {
        assert!(sms.supports_detached_speculation());
        assert!(sms.fork_detached_for_speculation().is_some());
    }

    let mut gg_left = backend(
        zeff_sega8_core::hardware::cartridge::SystemHint::GameGear,
        "left.gg",
    );
    let mut gg_right = backend(
        zeff_sega8_core::hardware::cartridge::SystemHint::GameGear,
        "right.gg",
    );
    assert!(gg_left.sync_link_peer(&mut gg_right));
    for game_gear in [&gg_left, &gg_right] {
        assert!(!game_gear.supports_detached_speculation());
        assert!(game_gear.fork_detached_for_speculation().is_none());
    }

    let mut sg1000 = backend(
        zeff_sega8_core::hardware::cartridge::SystemHint::Sg1000,
        "peer.sg",
    );
    assert!(!gg_left.sync_link_peer(&mut sg1000));
    assert!(!sg1000.supports_detached_speculation());
    assert!(sg1000.fork_detached_for_speculation().is_none());
}
