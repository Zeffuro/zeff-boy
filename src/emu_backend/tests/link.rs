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

#[cfg(not(target_arch = "wasm32"))]
mod wonder_swan_remote {
    use super::*;
    use crate::link::transport::LocalLinkTransport;
    use crate::link::ws::WonderSwanRemoteLink;
    use crate::link::{LinkEndpointId, LinkPacketKind, LinkSession, LinkSystemType};
    use zeff_emu_common::address::Address;
    use zeff_ws_core::hardware::constants::CYCLES_PER_FRAME;

    fn link_pair() -> (
        WonderSwanRemoteLink<LocalLinkTransport>,
        LinkSession<LocalLinkTransport>,
    ) {
        let (local, peer) = LocalLinkTransport::pair();
        (
            WonderSwanRemoteLink::new(LinkSession::new(
                local,
                LinkSystemType::WonderSwan,
                LinkEndpointId(1),
            )),
            LinkSession::new(peer, LinkSystemType::WonderSwan, LinkEndpointId(2)),
        )
    }

    fn send_watermark(peer: &mut LinkSession<LocalLinkTransport>, cycle: u64) {
        peer.send(LinkPacketKind::LinkState, &cycle.to_le_bytes())
            .unwrap();
    }

    #[test]
    fn stopped_peer_caps_actual_frame_execution_and_resumes_the_partial_frame() {
        let frame_cycles = u64::from(CYCLES_PER_FRAME);
        for watermark in [0, 1000] {
            let mut backend = build_ws_backend();
            let (mut link, mut peer) = link_pair();
            if watermark != 0 {
                send_watermark(&mut peer, watermark);
            }
            for _ in 0..3 {
                backend
                    .step_wonder_swan_frame_with_remote_link(&mut link)
                    .unwrap();
            }
            assert_eq!(backend.frame_count(), 2);
            let EmuBackend::Ws(ws) = &backend else {
                panic!("expected WonderSwan backend");
            };
            assert_eq!(ws.emu.cpu_cycles(), watermark + frame_cycles * 2 + 1);
            assert!(!ws.emu.frame_ready());
            let blocked_state = backend.encode_state_bytes().unwrap();
            for _ in 0..3 {
                backend
                    .step_wonder_swan_frame_with_remote_link(&mut link)
                    .unwrap();
                assert_eq!(backend.encode_state_bytes().unwrap(), blocked_state);
            }

            send_watermark(&mut peer, frame_cycles * 10);
            backend
                .step_wonder_swan_frame_with_remote_link(&mut link)
                .unwrap();
            assert_eq!(backend.frame_count(), 3);

            let mut reference = build_ws_backend();
            let (mut reference_link, mut reference_peer) = link_pair();
            send_watermark(&mut reference_peer, frame_cycles * 10);
            for _ in 0..3 {
                reference
                    .step_wonder_swan_frame_with_remote_link(&mut reference_link)
                    .unwrap();
            }
            assert_eq!(
                backend.encode_state_bytes().unwrap(),
                reference.encode_state_bytes().unwrap()
            );
            let (EmuBackend::Ws(ws), EmuBackend::Ws(reference)) = (&mut backend, &mut reference)
            else {
                panic!("expected WonderSwan backends");
            };
            let mut actual_audio = Vec::new();
            let mut expected_audio = Vec::new();
            ws.emu.drain_audio_samples_into(&mut actual_audio);
            reference.emu.drain_audio_samples_into(&mut expected_audio);
            assert!(!actual_audio.is_empty());
            assert_eq!(actual_audio, expected_audio);
        }
    }

    #[test]
    fn debugger_suspension_does_not_advance_or_complete_a_remote_frame() {
        for stop_before_step in [true, false] {
            let mut backend = build_ws_backend();
            let (mut link, mut peer) = link_pair();
            send_watermark(&mut peer, u64::from(CYCLES_PER_FRAME) * 10);
            let EmuBackend::Ws(ws) = &mut backend else {
                panic!("expected WonderSwan backend");
            };
            if stop_before_step {
                ws.emu.debug_suspend();
            } else {
                ws.emu
                    .add_one_shot_breakpoint(Address::from(ws.emu.cpu_pc()));
            }
            backend
                .step_wonder_swan_frame_with_remote_link(&mut link)
                .unwrap();
            assert_eq!(backend.frame_count(), 0);
            let EmuBackend::Ws(ws) = &backend else {
                panic!("expected WonderSwan backend");
            };
            assert_eq!(ws.emu.cpu_cycles(), 0);
            assert!(ws.emu.is_cpu_suspended());
            let stopped_state = backend.encode_state_bytes().unwrap();
            backend
                .step_wonder_swan_frame_with_remote_link(&mut link)
                .unwrap();
            assert_eq!(backend.encode_state_bytes().unwrap(), stopped_state);

            let EmuBackend::Ws(ws) = &mut backend else {
                panic!("expected WonderSwan backend");
            };
            ws.emu.debug_continue();
            backend
                .step_wonder_swan_frame_with_remote_link(&mut link)
                .unwrap();
            assert_eq!(backend.frame_count(), 1);
        }
    }
}
