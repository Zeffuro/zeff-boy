use super::*;
use crate::debug::DebugUiActions;
#[cfg(not(target_arch = "wasm32"))]
use crate::link::LinkSession;
#[cfg(not(target_arch = "wasm32"))]
use std::net::TcpListener;
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::thread;
#[cfg(not(target_arch = "wasm32"))]
use zeff_gb_core::hardware::types::constants::{INTERRUPT_IF, SERIAL_SB, SERIAL_SC};
use zeff_ws_core::hardware::cartridge::compute_footer_checksum;

fn minimal_ws_rom() -> Vec<u8> {
    let mut rom = vec![0xFF; 0x10000];
    let reset_vector = rom.len() - 16;
    rom[reset_vector..reset_vector + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    rom[0] = 0xF4;
    let footer = rom.len() - 10;
    rom[footer] = 0x01;
    rom[footer + 1] = 0x00;
    rom[footer + 2] = 0x23;
    rom[footer + 4] = 0x01;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

#[test]
fn build_frame_result_does_not_republish_stale_recycled_audio() {
    let rom = minimal_ws_rom();
    let emu = zeff_ws_core::emulator::Emulator::from_rom_data(&rom).unwrap();
    let mut backend = EmuBackend::from_ws(emu, PathBuf::from("test.wsc"));
    let mut runtime_fault = WorkerRuntimeFault::default();

    let result = EmuThread::build_frame_result(
        &mut backend,
        &mut runtime_fault,
        Some(vec![0.25, -0.25, 0.5, -0.5]),
        ui::UiFrameData::default(),
        Vec::new(),
        0.0,
        0,
        1,
    );

    assert!(
        result.audio_samples.is_empty(),
        "stale recycled audio samples must be cleared before draining new core audio"
    );
    assert_eq!(result.audio_playback_speed, 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn load_state_response_reports_authoritative_game_boy_serial_device() {
    let mut backend = gb_backend();
    backend.set_game_boy_serial_device(
        zeff_gb_core::hardware::GameBoySerialDevice::BardigunBarcodeReader,
    );
    let shared_fb: SharedFramebuffer = Default::default();

    let response = EmuThread::respond_load_state(
        &mut backend,
        Ok(()),
        "test state".to_string(),
        0,
        0,
        &shared_fb,
    );

    assert!(matches!(
        response,
        EmuResponse::LoadStateOk {
            game_boy_serial_device: Some(
                zeff_gb_core::hardware::GameBoySerialDevice::BardigunBarcodeReader
            ),
            ..
        }
    ));
}

#[test]
fn load_state_response_has_no_game_boy_serial_device_for_other_systems() {
    let rom = minimal_ws_rom();
    let emu = zeff_ws_core::emulator::Emulator::from_rom_data(&rom).unwrap();
    let mut backend = EmuBackend::from_ws(emu, PathBuf::from("test.wsc"));
    let shared_fb: SharedFramebuffer = Default::default();

    let response = EmuThread::respond_load_state(
        &mut backend,
        Ok(()),
        "test state".to_string(),
        0,
        0,
        &shared_fb,
    );

    assert!(matches!(
        response,
        EmuResponse::LoadStateOk {
            game_boy_serial_device: None,
            ..
        }
    ));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn rewind_response_reports_serial_device_restored_from_state() {
    let mut backend = gb_backend();
    backend.set_game_boy_serial_device(
        zeff_gb_core::hardware::GameBoySerialDevice::BardigunBarcodeReader,
    );
    let state = EmuThread::encode_current_state(&backend).unwrap();
    backend.set_game_boy_serial_device(zeff_gb_core::hardware::GameBoySerialDevice::Disconnected);
    let mut rewind_buffer = zeff_emu_common::rewind::RewindBuffer::new(1, 1);
    rewind_buffer.push(&state, &[]);
    let shared_fb: SharedFramebuffer = Default::default();

    let response = EmuThread::handle_rewind(&mut backend, &mut rewind_buffer, &shared_fb, 1);

    assert!(matches!(
        response,
        EmuResponse::RewindOk {
            game_boy_serial_device: Some(
                zeff_gb_core::hardware::GameBoySerialDevice::BardigunBarcodeReader
            ),
            ..
        }
    ));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn rewind_keeps_same_frame_snapshot_when_current_state_differs() {
    let mut backend = gb_backend();
    let older_state = EmuThread::encode_current_state(&backend).unwrap();
    backend.set_game_boy_serial_device(
        zeff_gb_core::hardware::GameBoySerialDevice::BardigunBarcodeReader,
    );
    let latest_state = EmuThread::encode_current_state(&backend).unwrap();
    backend.set_game_boy_serial_device(zeff_gb_core::hardware::GameBoySerialDevice::Disconnected);

    let mut rewind_buffer = zeff_emu_common::rewind::RewindBuffer::new(1, 1);
    rewind_buffer.push(&older_state, &[]);
    rewind_buffer.push(&latest_state, &[]);
    let shared_fb: SharedFramebuffer = Default::default();

    let response = EmuThread::handle_rewind(&mut backend, &mut rewind_buffer, &shared_fb, 1);

    assert!(matches!(
        response,
        EmuResponse::RewindOk {
            game_boy_serial_device: Some(
                zeff_gb_core::hardware::GameBoySerialDevice::BardigunBarcodeReader
            ),
            ..
        }
    ));
}

#[test]
fn step_n_frames_collects_midi_snapshot_per_emulated_frame() {
    let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &[0x00],
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .unwrap();
    let mut backend = EmuBackend::from_sega8(emu, PathBuf::from("test.sms"));
    let mut audio_semantic_frames = Vec::new();

    let advanced_frames =
        EmuThread::step_n_frames(&mut backend, 3, &[], true, &mut audio_semantic_frames, None);

    assert_eq!(backend.frame_count(), 3);
    assert_eq!(advanced_frames, 3);
    assert_eq!(audio_semantic_frames.len(), 3);
}

#[test]
fn gbc_rewind_cadence_tracks_advanced_frames_across_batches() {
    let rom = vec![0; 0x8000];
    let gb = zeff_gb_core::emulator::Emulator::from_rom_data(
        &rom,
        zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::ForceCgb,
    )
    .unwrap();
    let mut backend = EmuBackend::from_gb(gb, PathBuf::from("test.gbc"));
    let mut rewind = zeff_emu_common::rewind::RewindBuffer::new(1, 4);
    let mut semantic = Vec::new();

    for frames in [2, 3, 1, 3] {
        let advanced =
            EmuThread::step_n_frames(&mut backend, frames, &[], false, &mut semantic, None);
        EmuThread::capture_rewind_snapshot(&backend, &mut rewind, true, advanced);
    }
    assert_eq!(rewind.len(), 2);

    let shared_fb: SharedFramebuffer = Default::default();
    let response = EmuThread::handle_rewind(&mut backend, &mut rewind, &shared_fb, 1);
    assert!(matches!(
        response,
        EmuResponse::RewindOk {
            rewound_frames: 4,
            ..
        }
    ));
}

#[test]
fn next_frame_resumes_for_one_frame_then_suspends() {
    let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &[0x00],
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .unwrap();
    let mut backend = EmuBackend::from_sega8(emu, PathBuf::from("test.sms"));
    backend.debug_suspend();

    let actions = DebugUiActions::none();
    let mut config = BackendRuntimeConfig::new(&actions);
    config.debug_continue = true;
    backend.apply_runtime_config(config);
    let before = backend.frame_count();
    let advanced = EmuThread::step_n_frames(&mut backend, 1, &[], false, &mut Vec::new(), None);
    EmuThread::suspend_after_debug_frame(&mut backend, true, advanced);

    assert_eq!(advanced, 1);
    assert_eq!(backend.frame_count(), before + 1);
    assert!(backend.is_suspended());
}

#[test]
fn step_n_frames_applies_replay_joypad_input_per_emulated_frame() {
    let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
        &[0x00],
        44_100,
        zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
    )
    .unwrap();
    let mut backend = EmuBackend::from_sega8(emu, PathBuf::from("test.sms"));
    let replay_frames = [
        ReplayJoypadFrame {
            buttons: 0x01,
            dpad: 0x00,
            buttons_p2: 0x02,
            dpad_p2: 0x00,
            zapper: Default::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: None,
            ..ReplayJoypadFrame::default()
        },
        ReplayJoypadFrame {
            buttons: 0x00,
            dpad: 0x01,
            buttons_p2: 0x00,
            dpad_p2: 0x02,
            zapper: Default::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: None,
            ..ReplayJoypadFrame::default()
        },
    ];
    let mut audio_semantic_frames = Vec::new();

    let advanced_frames = EmuThread::step_n_frames(
        &mut backend,
        2,
        &[],
        false,
        &mut audio_semantic_frames,
        Some(&replay_frames),
    );

    assert_eq!(advanced_frames, 2);
    let EmuBackend::Sega8(sega8) = &backend else {
        panic!("expected Sega8 backend");
    };
    let raw = sega8
        .emu
        .bus()
        .input()
        .read_controller(zeff_sega8_core::hardware::input::ControllerPort::One);
    assert_ne!(raw & (1 << 4), 0, "button 1 should be released");
    assert_eq!(raw & (1 << 3), 0, "right should be pressed");

    let raw_p2 = sega8
        .emu
        .bus()
        .input()
        .read_controller(zeff_sega8_core::hardware::input::ControllerPort::Two);
    assert_ne!(raw_p2 & (1 << 5), 0, "P2 button 2 should be released");
    assert_eq!(raw_p2 & (1 << 2), 0, "P2 left should be pressed");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn tcp_link_stepper_exchanges_game_boy_bytes_between_backends() {
    let (mut left_link, mut right_link) = tcp_link_pair();
    let mut left = gb_backend();
    let mut right = gb_backend();

    {
        let (EmuBackend::Gb(left), EmuBackend::Gb(right)) = (&mut left, &mut right) else {
            panic!("expected GB backends");
        };
        left.emu.write_byte(SERIAL_SB, 0xAB);
        right.emu.write_byte(SERIAL_SB, 0x34);
        left.emu.write_byte(SERIAL_SC, 0x81);
        right.emu.write_byte(SERIAL_SC, 0x80);
    }

    for _ in 0..50 {
        let mut left_snapshots = Vec::new();
        EmuThread::step_n_frames_with_tcp_link(
            &mut left,
            1,
            &[],
            Some(&mut left_link),
            false,
            &mut left_snapshots,
            None,
        );
        let mut right_snapshots = Vec::new();
        EmuThread::step_n_frames_with_tcp_link(
            &mut right,
            1,
            &[],
            Some(&mut right_link),
            false,
            &mut right_snapshots,
            None,
        );
        let (EmuBackend::Gb(left_gb), EmuBackend::Gb(right_gb)) = (&left, &right) else {
            panic!("expected GB backends");
        };
        if left_gb.emu.cpu_peek8(SERIAL_SC) & 0x80 == 0
            && right_gb.emu.cpu_peek8(SERIAL_SC) & 0x80 == 0
        {
            break;
        }
        std::thread::yield_now();
    }

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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn tcp_link_stepper_stops_after_game_boy_master_start_until_peer_replies() {
    let (mut left_link, _right_link) = tcp_link_pair();
    let mut left = gb_backend();

    {
        let EmuBackend::Gb(left) = &mut left else {
            panic!("expected GB backend");
        };
        left.emu.write_byte(SERIAL_SB, 0xAB);
        left.emu.write_byte(SERIAL_SC, 0x81);
    }

    let mut audio_semantic_frames = Vec::new();
    let advanced_frames = EmuThread::step_n_frames_with_tcp_link(
        &mut left,
        5,
        &[],
        Some(&mut left_link),
        true,
        &mut audio_semantic_frames,
        None,
    );

    let EmuBackend::Gb(left) = &left else {
        panic!("expected GB backend");
    };
    assert_eq!(left.emu.frame_count(), 0);
    assert_eq!(advanced_frames, 0);
    assert!(
        audio_semantic_frames.is_empty(),
        "GB link waits must not record audio-tooling time when no emulated frame advanced"
    );
    assert_eq!(left.emu.cpu_peek8(SERIAL_SC) & 0x80, 0x80);
    assert_eq!(
        left.emu.game_boy_link_state().pending_master_byte,
        Some(0xAB)
    );
    assert!(
        !left.emu.game_boy_link_waiting_at_completion_boundary(),
        "live TCP stepping must yield before the serial completion boundary"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn tcp_link_stepper_reuses_replay_input_when_game_boy_link_slice_does_not_advance_frame() {
    let (mut left_link, _right_link) = tcp_link_pair();
    let mut left = gb_backend();
    let replay_frames = [
        ReplayJoypadFrame {
            buttons: 0x01,
            dpad: 0x00,
            buttons_p2: 0,
            dpad_p2: 0,
            zapper: Default::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: None,
            ..ReplayJoypadFrame::default()
        },
        ReplayJoypadFrame {
            buttons: 0x02,
            dpad: 0x00,
            buttons_p2: 0,
            dpad_p2: 0,
            zapper: Default::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: None,
            ..ReplayJoypadFrame::default()
        },
    ];

    {
        let EmuBackend::Gb(left) = &mut left else {
            panic!("expected GB backend");
        };
        left.emu.write_byte(SERIAL_SB, 0xAB);
        left.emu.write_byte(SERIAL_SC, 0x81);
    }

    let mut audio_semantic_frames = Vec::new();
    let advanced_frames = EmuThread::step_n_frames_with_tcp_link(
        &mut left,
        2,
        &[],
        Some(&mut left_link),
        false,
        &mut audio_semantic_frames,
        Some(&replay_frames),
    );

    assert_eq!(advanced_frames, 0);
    let EmuBackend::Gb(left) = &mut left else {
        panic!("expected GB backend");
    };
    left.emu.write_byte(0xFF00, 0x10);
    assert_eq!(
        left.emu.cpu_peek8(0xFF00) & 0x0F,
        0x0E,
        "second replay input must not be applied until an emulated frame advances"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn tcp_link_stepper_does_not_stop_at_game_boy_external_clock_wait() {
    let (mut left_link, _right_link) = tcp_link_pair();
    let mut left = gb_backend();

    {
        let EmuBackend::Gb(left) = &mut left else {
            panic!("expected GB backend");
        };
        left.emu.write_byte(SERIAL_SB, 0x02);
        left.emu.write_byte(SERIAL_SC, 0x80);
    }

    let mut audio_semantic_frames = Vec::new();
    let advanced_frames = EmuThread::step_n_frames_with_tcp_link(
        &mut left,
        1,
        &[],
        Some(&mut left_link),
        false,
        &mut audio_semantic_frames,
        None,
    );

    let EmuBackend::Gb(left) = &left else {
        panic!("expected GB backend");
    };
    assert!(advanced_frames > 0);
    assert!(
        left.emu.frame_count() > 0,
        "external-clock readiness must not deadlock the frame stepper"
    );
    assert_eq!(left.emu.cpu_peek8(SERIAL_SC) & 0x80, 0x80);
    assert_eq!(
        left.emu.game_boy_link_state().external_clock_byte,
        Some(0x02)
    );
    assert_eq!(left.emu.game_boy_link_state().pending_master_byte, None);
}

#[cfg(not(target_arch = "wasm32"))]
fn tcp_link_pair() -> (
    crate::link::RemoteLink<crate::link::transport::TcpLinkTransport>,
    crate::link::RemoteLink<crate::link::transport::TcpLinkTransport>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let host_thread = thread::spawn(move || {
        crate::link::transport::TcpLinkTransport::accept_once(listener).unwrap()
    });

    let client = crate::link::transport::TcpLinkTransport::connect(addr).unwrap();
    let host = host_thread.join().unwrap();

    (
        crate::link::RemoteLink::GameBoy(crate::link::gb::GameBoyRemoteLink::new(
            LinkSession::new(
                host,
                crate::link::LinkSystemType::GameBoy,
                crate::link::LinkEndpointId(1),
            ),
        )),
        crate::link::RemoteLink::GameBoy(crate::link::gb::GameBoyRemoteLink::new(
            LinkSession::new(
                client,
                crate::link::LinkSystemType::GameBoy,
                crate::link::LinkEndpointId(2),
            ),
        )),
    )
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn tcp_link_stepper_exchanges_wonder_swan_uart_bytes_between_backends() {
    let (mut left_link, mut right_link) = tcp_wonder_swan_link_pair();
    let mut left = ws_backend();
    let mut right = ws_backend();

    {
        let (EmuBackend::Ws(left), EmuBackend::Ws(right)) = (&mut left, &mut right) else {
            panic!("expected WS backends");
        };
        left.emu.io_write8(0x00B3, 0x80);
        right.emu.io_write8(0x00B3, 0x80);
        left.emu.io_write8(0x00B1, 0x5A);
    }

    for _ in 0..50 {
        let mut left_snapshots = Vec::new();
        EmuThread::step_n_frames_with_tcp_link(
            &mut left,
            1,
            &[],
            Some(&mut left_link),
            false,
            &mut left_snapshots,
            None,
        );
        let mut right_snapshots = Vec::new();
        EmuThread::step_n_frames_with_tcp_link(
            &mut right,
            1,
            &[],
            Some(&mut right_link),
            false,
            &mut right_snapshots,
            None,
        );
        let EmuBackend::Ws(right_ws) = &right else {
            panic!("expected WS backend");
        };
        if right_ws.emu.io_peek8(0x00B3) & 0x01 != 0 {
            break;
        }
        std::thread::yield_now();
    }

    let EmuBackend::Ws(right) = &right else {
        panic!("expected WS backend");
    };
    assert_eq!(right.emu.io_peek8(0x00B3) & 0x01, 0x01);
    assert_eq!(right.emu.io_peek8(0x00B1), 0x5A);
}

#[cfg(not(target_arch = "wasm32"))]
fn tcp_wonder_swan_link_pair() -> (
    crate::link::RemoteLink<crate::link::transport::TcpLinkTransport>,
    crate::link::RemoteLink<crate::link::transport::TcpLinkTransport>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let host_thread = thread::spawn(move || {
        crate::link::transport::TcpLinkTransport::accept_once(listener).unwrap()
    });

    let client = crate::link::transport::TcpLinkTransport::connect(addr).unwrap();
    let host = host_thread.join().unwrap();

    (
        crate::link::RemoteLink::WonderSwan(crate::link::ws::WonderSwanRemoteLink::new(
            LinkSession::new(
                host,
                crate::link::LinkSystemType::WonderSwan,
                crate::link::LinkEndpointId(1),
            ),
        )),
        crate::link::RemoteLink::WonderSwan(crate::link::ws::WonderSwanRemoteLink::new(
            LinkSession::new(
                client,
                crate::link::LinkSystemType::WonderSwan,
                crate::link::LinkEndpointId(2),
            ),
        )),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn ws_backend() -> EmuBackend {
    let ws = zeff_ws_core::emulator::Emulator::from_rom_data(&minimal_running_ws_rom())
        .expect("WS emulator should initialize");
    EmuBackend::from_ws(ws, PathBuf::from("test.ws"))
}

fn minimal_running_ws_rom() -> Vec<u8> {
    let mut rom = vec![0x90; 0x10000];
    rom[0] = 0x90;
    rom[1] = 0xEB;
    rom[2] = 0xFC;
    let reset_vector = rom.len() - 16;
    rom[reset_vector..reset_vector + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer] = 0x01;
    rom[footer + 1] = 0x00;
    rom[footer + 2] = 0x23;
    rom[footer + 4] = 0x01;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

#[cfg(not(target_arch = "wasm32"))]
fn gb_backend() -> EmuBackend {
    let rom = vec![0u8; 0x8000];
    let gb =
        zeff_gb_core::emulator::Emulator::new(&rom, 44_100).expect("GB emulator should initialize");
    EmuBackend::from_gb(gb, PathBuf::from("test.gb"))
}
