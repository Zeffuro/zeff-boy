use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use std::net::{TcpListener, TcpStream};

fn reject_high_bits(mut backend: EmuBackend, mut peer: EmuBackend) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (b, _) = listener.accept().unwrap();
    let start = |stream: TcpStream, player| Start {
        stream: stream.into(),
        player,
        build: [7; 32],
        secret: [5; 32],
        scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
        allow_different_versions: false,
        verify_every_frame: true,
        input_delay: zeff_netplay::rollback::InputDelay::new(0).unwrap(),
    };
    let mut local = Session::start(&mut backend, start(a, Player::One)).unwrap();
    let mut remote = Session::start(&mut peer, start(b, Player::Two)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !local.ready || !remote.ready {
        local.poll(&mut backend).unwrap();
        remote.poll(&mut peer).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let before = backend.encode_state_bytes().unwrap();
    let confirmed = local.timeline.confirmed_frame();
    let retained = local.timeline.retained_inputs();
    for buttons in [0x0100, 0x0401, 0x8000, u16::MAX] {
        assert!(
            local
                .step(&mut backend, buttons)
                .unwrap_err()
                .to_string()
                .contains("input bits")
        );
        assert!(
            local
                .handle_event(Event::Message(Message::Input {
                    player: Player::Two,
                    frame: 0,
                    buttons,
                }))
                .unwrap_err()
                .to_string()
                .contains("input bits")
        );
        assert!(local.lease.advance(&mut backend, [buttons, 0]).is_err());
        assert!(local.lease.advance(&mut backend, [0, buttons]).is_err());
        assert_eq!(local.timeline.frame(), 0);
        assert_eq!(local.timeline.confirmed_frame(), confirmed);
        assert_eq!(local.timeline.retained_inputs(), retained);
        assert_eq!(local.sampled, None);
        assert_eq!(backend.encode_state_bytes().unwrap(), before);
        assert_eq!(remote.timeline.retained_inputs(), retained);
    }
    local.lease.validate_input(0xff).unwrap();
    local.restore(&mut backend).unwrap();
    remote.restore(&mut peer).unwrap();
}

#[test]
fn supported_cores_reject_wide_local_remote_and_execution_inputs() {
    let directory = crate::test_support::test_directory("netplay-input-width").unwrap();
    let path = directory.path().join("game.nes");
    std::fs::write(
        &path,
        super::super::proof::fixture_rom(zeff_nes_core::hardware::cartridge::TimingMode::Ntsc),
    )
    .unwrap();
    let nes = || {
        load_backend_from_rom_source(
            ActiveSystem::Nes,
            &path,
            &path,
            None,
            BackendLoadConfig {
                nes_load_battery_sram: false,
                ..Default::default()
            },
        )
        .unwrap()
        .backend
    };
    reject_high_bits(nes(), nes());
    for system in [ActiveSystem::MasterSystem, ActiveSystem::Sg1000] {
        let (_a, a) = sega8_tests::loaded(system, false);
        let (_b, b) = sega8_tests::loaded(system, false);
        reject_high_bits(a, b);
    }
    for supergrafx in [false, true] {
        let (_a, a) = pce_tests::loaded(supergrafx);
        let (_b, b) = pce_tests::loaded(supergrafx);
        reject_high_bits(a, b);
    }
}
