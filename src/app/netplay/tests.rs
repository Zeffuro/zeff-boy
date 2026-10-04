use super::*;
use crate::app::keyboard::HeldFrontendAction;
use crate::app::tas_control::tests::harness::app_with_worker;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use crate::emu_thread::{EmuResponsePoll, EmuThread, TasExecutionProfile};
use crate::input::HostButton;
use sha2::{Digest, Sha256};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;

pub(super) fn load(path: &Path) -> EmuBackend {
    load_backend_from_rom_source(
        ActiveSystem::Nes,
        path,
        path,
        None,
        BackendLoadConfig {
            apply_mods: true,
            sample_rate: Some(48_000),
            nes_load_battery_sram: true,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend
}

pub(super) fn app(root: &Path, name: &str) -> App {
    app_with_timing(
        root,
        name,
        zeff_nes_core::hardware::cartridge::TimingMode::Ntsc,
    )
}

pub(super) fn app_with_timing(
    root: &Path,
    name: &str,
    timing: zeff_nes_core::hardware::cartridge::TimingMode,
) -> App {
    let path = root.join(format!("{name}.nes"));
    let rom = crate::netplay::proof::fixture_rom(timing);
    std::fs::write(&path, rom).unwrap();
    let worker = EmuThread::spawn(load(&path), false);
    app_with_worker(worker, 11, ActiveSystem::Nes, path)
}

pub(super) fn wait(app: &mut App, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate(app) {
        app.drain_emu_responses();
        assert!(
            Instant::now() < deadline,
            "App response deadline: {}",
            app.debug_windows.netplay.status
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub(super) fn capture(app: &mut App) -> Vec<u8> {
    app.emu_thread
        .as_ref()
        .unwrap()
        .send(EmuCommand::CaptureStateBytes);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match app.emu_thread.as_ref().unwrap().poll_response() {
            EmuResponsePoll::Response(response) => match *response {
                EmuResponse::StateCaptured(bytes) => return bytes,
                other => {
                    app.consume_netplay_response(other);
                }
            },
            EmuResponsePoll::Disconnected => panic!("worker disconnected during capture"),
            EmuResponsePoll::Empty => {}
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn hosting(app: &mut App) -> String {
    app.begin_netplay(None).unwrap();
    app.pump_netplay();
    assert!(
        app.netplay.phase == Phase::Connecting,
        "{}",
        app.debug_windows.netplay.status
    );
    assert!(!app.debug_windows.netplay.invitation.is_empty());
    app.debug_windows.netplay.invitation.clone()
}

pub(super) fn connect_pair(one: &mut App, two: &mut App) -> (Vec<u8>, Vec<u8>) {
    let invitation = hosting(one);
    let initial_one = capture(one);
    two.begin_netplay(Some(invitation)).unwrap();
    let initial_two = capture(two);
    two.pump_netplay();
    assert!(
        matches!(two.netplay.phase, Phase::Connecting | Phase::Admission),
        "{}",
        two.debug_windows.netplay.status
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !one.netplay.running() || !two.netplay.running() {
        for app in [&mut *one, &mut *two] {
            if !app.netplay.running() {
                app.pump_netplay();
            }
            app.drain_emu_responses();
            assert!(
                app.netplay.fenced(),
                "admission failed: {}",
                app.debug_windows.netplay.status
            );
        }
        assert!(Instant::now() < deadline, "pair admission deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    for app in [&mut *one, &mut *two] {
        app.netplay.next_frame = Some(Instant::now() + Duration::from_secs(60));
        assert_eq!(app.netplay.confirmed, 0);
        assert!(!app.netplay.in_flight);
        assert!(app.netplay.observed_frames.is_empty());
        assert!(app.netplay.queued_audio.is_none());
    }
    (initial_one, initial_two)
}

pub(super) fn set_input(app: &mut App, raw: u8) {
    app.host_input.clear_keyboard();
    for (bit, button) in [
        HostButton::A,
        HostButton::B,
        HostButton::Select,
        HostButton::Start,
        HostButton::Up,
        HostButton::Down,
        HostButton::Left,
        HostButton::Right,
    ]
    .into_iter()
    .enumerate()
    {
        app.host_input.set_keyboard(button, raw & (1 << bit) != 0);
        app.host_input.set_keyboard_p2(button, true);
    }
}

pub(super) fn audio_bits(samples: &[f32]) -> Vec<u32> {
    samples.iter().map(|sample| sample.to_bits()).collect()
}

#[test]
fn preparing_and_connecting_cancel_and_game_teardown_release_listener() {
    let directory = crate::test_support::test_directory("app-netplay-cancel").unwrap();
    let mut app = app(directory.path(), "one");
    app.begin_netplay(None).unwrap();
    assert!(matches!(app.netplay.phase, Phase::Preparing(_)));
    app.request_netplay_stop();
    assert!(app.netplay.phase == Phase::Idle);
    assert!(app.speed.paused);
    for teardown in [false, true] {
        let invitation = hosting(&mut app);
        let address: SocketAddr = invitation.split_once('/').unwrap().0.parse().unwrap();
        let start = Instant::now();
        if teardown {
            app.stop_game();
        } else {
            app.request_netplay_stop();
        }
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(app.netplay.phase == Phase::Idle);
        assert!(app.netplay.connector.is_none());
        assert!(TcpListener::bind(address).is_ok());
        if teardown {
            assert!(app.emu_thread.is_none());
        } else {
            assert!(app.speed.paused);
        }
    }
}

#[test]
fn admission_cancel_restores_real_worker_and_save() {
    let directory = crate::test_support::test_directory("app-netplay-handshake").unwrap();
    let mut app = app(directory.path(), "one");
    let invitation = hosting(&mut app);
    let initial = capture(&mut app);
    let path = app.rom_info.rom_path.clone().unwrap().with_extension("sav");
    let baseline = std::fs::read(&path).unwrap();
    let address: SocketAddr = invitation.split_once('/').unwrap().0.parse().unwrap();
    let silent_peer = TcpStream::connect(address).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while app.netplay.phase == Phase::Connecting {
        app.pump_netplay();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(app.netplay.phase == Phase::Admission);
    let start = Instant::now();
    app.request_netplay_stop();
    wait(&mut app, |app| app.netplay.phase == Phase::Idle);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(app.speed.paused);
    assert_eq!(capture(&mut app), initial);
    assert_eq!(std::fs::read(&path).unwrap(), baseline);
    drop(silent_peer);
    app.stop_emu_thread();
    assert_eq!(std::fs::read(&path).unwrap(), baseline);
}

#[test]
fn active_session_fences_mutations_and_tas_without_affecting_input() {
    let directory = crate::test_support::test_directory("app-netplay-fences").unwrap();
    let mut one = app(directory.path(), "one");
    let mut two = app(directory.path(), "two");
    connect_pair(&mut one, &mut two);
    set_input(&mut one, 0x81);
    one.fence_tas_control_gameplay();
    for action in [
        HeldFrontendAction::FastForward,
        HeldFrontendAction::Turbo,
        HeldFrontendAction::Rewind,
    ] {
        one.set_gamepad_frontend_hold(action, true);
        one.set_remote_frontend_hold(action, true);
        assert!(!one.speed.fast_forward_held);
        assert!(!one.speed.turbo_held);
        assert!(!one.rewind.held);
        one.set_gamepad_frontend_hold(action, false);
        one.set_remote_frontend_hold(action, false);
    }
    let generation = one.emu_worker_generation;
    let source = one.rom_info.source_path.clone();
    let symbol_request = one.next_symbol_load_id;
    one.load_rom(&directory.path().join("missing-cartridge.nes"));
    one.open_file_dialog();
    one.open_symbol_file_dialog();
    assert_eq!(one.emu_worker_generation, generation);
    assert_eq!(one.rom_info.source_path, source);
    assert_eq!(one.next_symbol_load_id, symbol_request);
    assert!(one.pending_rom_preparation.is_none());
    assert!(one.netplay.phase == Phase::Running);
    assert!(!one.speed.paused);
    assert_eq!(one.current_host_joypad_input(), (1, 1));
    assert!(one.begin_netplay(None).is_err());
    assert!(one.begin_tas_control_acquire().is_err());
    for command in [
        EmuCommand::Reset,
        EmuCommand::SetSampleRate(44_100),
        EmuCommand::CaptureStateBytes,
        EmuCommand::SaveStateToPath(directory.path().join("forbidden.state")),
        EmuCommand::AcquireTasControl {
            request_id: 9,
            profile: TasExecutionProfile::DirectNesCartridge,
        },
    ] {
        assert!(one.send_emu_command_checked(command).is_err());
    }
    one.tick();
    assert_eq!(one.frames_in_flight, 0);
    assert_eq!(one.netplay.confirmed, 0);
    assert_eq!(one.current_host_joypad_input(), (1, 1));
    assert!(!directory.path().join("forbidden.state").exists());
    one.request_netplay_stop();
    two.request_netplay_stop();
    wait(&mut one, |app| app.netplay.phase == Phase::Idle);
    wait(&mut two, |app| app.netplay.phase == Phase::Idle);
    one.stop_emu_thread();
    two.stop_emu_thread();
}

#[test]
fn failed_restore_response_poison_prevents_resume_until_game_teardown() {
    let directory = crate::test_support::test_directory("app-netplay-poison").unwrap();
    let mut app = app(directory.path(), "one");
    app.netplay.phase = Phase::Admission;
    assert!(
        app.consume_netplay_response(EmuResponse::Netplay(Response::Stopped {
            reason: "restoration failure".into(),
            restored: false,
        }))
        .is_none()
    );
    assert!(app.netplay.phase == Phase::Poisoned);
    assert!(app.netplay.fenced());
    assert!(app.speed.paused);
    app.set_user_paused(false);
    app.toggle_user_paused();
    assert!(app.speed.paused);
    assert!(app.begin_netplay(None).is_err());
    assert!(app.send_emu_command_checked(EmuCommand::Reset).is_err());
    app.tick();
    assert_eq!(app.frames_in_flight, 0);
    assert_eq!(app.netplay.confirmed, 0);
    app.stop_game();
    assert!(app.netplay.phase == Phase::Idle);
    assert!(app.emu_thread.is_none());
}

#[test]
fn malformed_join_is_rejected_before_reload_pause_or_save_changes() {
    let directory = crate::test_support::test_directory("app-netplay-join-failure").unwrap();
    let mut app = app(directory.path(), "one");
    let initial = capture(&mut app);
    let generation = app.emu_worker_generation;
    assert!(
        app.begin_netplay(Some("192.168.1.1:1234/invalid".into()))
            .is_err()
    );
    assert!(app.netplay.phase == Phase::Idle);
    assert!(!app.speed.paused);
    assert_eq!(app.emu_worker_generation, generation);
    assert!(app.netplay.connector.is_none());
    assert_eq!(app.netplay.confirmed, 0);
    let path = app.rom_info.rom_path.clone().unwrap();
    assert_eq!(capture(&mut app), initial);
    assert!(!path.with_extension("sav").exists());
    let baseline = Sha256::digest(
        load(&path)
            .nes()
            .unwrap()
            .emu
            .dump_persistent_data()
            .unwrap(),
    );
    app.stop_emu_thread();
    assert_eq!(
        Sha256::digest(std::fs::read(path.with_extension("sav")).unwrap()),
        baseline
    );
}

#[test]
fn wrong_capability_admission_failure_restores_both_apps_without_frames() {
    let directory = crate::test_support::test_directory("app-netplay-auth-failure").unwrap();
    let mut one = app(directory.path(), "one");
    let mut two = app(directory.path(), "two");
    let invitation = hosting(&mut one);
    let initial_one = capture(&mut one);
    let mut invitation = invitation.into_bytes();
    let capability = invitation.iter().position(|&byte| byte == b'/').unwrap() + 1;
    let byte = &mut invitation[capability];
    *byte = if *byte == b'0' { b'1' } else { b'0' };
    two.begin_netplay(Some(String::from_utf8(invitation).unwrap()))
        .unwrap();
    let initial_two = capture(&mut two);
    two.pump_netplay();
    let paths = [
        one.rom_info.rom_path.clone().unwrap(),
        two.rom_info.rom_path.clone().unwrap(),
    ];
    let baseline = paths
        .each_ref()
        .map(|path| std::fs::read(path.with_extension("sav")).unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while one.netplay.fenced() || two.netplay.fenced() {
        for app in [&mut one, &mut two] {
            app.pump_netplay();
            app.drain_emu_responses();
            assert!(!app.netplay.running());
            assert!(app.netplay.phase != Phase::Poisoned);
        }
        assert!(Instant::now() < deadline, "authentication failure deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    for (app, initial) in [(&mut one, &initial_one), (&mut two, &initial_two)] {
        assert!(app.speed.paused);
        assert_eq!(app.netplay.confirmed, 0);
        assert!(app.debug_windows.netplay.status.contains("authentication"));
        assert_eq!(capture(app), *initial);
        app.stop_emu_thread();
    }
    for (path, expected) in paths.iter().zip(&baseline) {
        assert_eq!(
            &std::fs::read(path.with_extension("sav")).unwrap(),
            expected
        );
    }
}

#[test]
fn start_rejection_then_immediate_cancel_finishes_after_both_worker_rejections() {
    let directory = crate::test_support::test_directory("app-netplay-start-rejected").unwrap();
    let path = directory.path().join("one.nes");
    std::fs::write(&path, zeff_netplay::fixture::rom()).unwrap();
    let mut backend = load(&path);
    backend.step_frame();
    let expected = backend.encode_state_bytes().unwrap();
    let worker = EmuThread::spawn(backend, false);
    let mut app = app_with_worker(worker, 11, ActiveSystem::Nes, path);
    let (mut connector, invitation) = Connector::host(HostOptions {
        input_delay: zeff_netplay::rollback::InputDelay::default(),
        address: "127.0.0.1:0".parse().unwrap(),
        scope: ConnectionScope::Loopback,
    })
    .unwrap();
    let mut peer = Connector::join(&invitation, ConnectionScope::Loopback).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let start = loop {
        if let Some(start) = connector.poll().unwrap() {
            break start;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    let _peer_start = loop {
        if let Some(start) = peer.poll().unwrap() {
            break start;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    app.netplay.phase = Phase::Connecting;
    app.send_emu_command_checked(EmuCommand::StartNetplay(Box::new(start)))
        .unwrap();
    app.netplay.phase = Phase::Admission;
    app.request_netplay_stop();
    assert!(app.netplay.phase == Phase::Stopping);
    let first = next_response(&app);
    assert!(
        matches!(&first, EmuResponse::Netplay(Response::Rejected(reason)) if reason.contains("unexecuted"))
    );
    app.consume_netplay_response(first);
    assert!(app.netplay.phase == Phase::Idle);
    assert!(app.speed.paused);
    assert!(!app.netplay.fenced());
    app.begin_netplay(None).unwrap();
    assert!(matches!(
        app.netplay.phase,
        Phase::Preparing(Request::Host(..))
    ));
    let second = next_response(&app);
    assert!(
        matches!(&second, EmuResponse::Netplay(Response::Rejected(reason)) if reason == "no netplay session")
    );
    app.consume_netplay_response(second);
    app.drain_emu_responses();
    assert!(matches!(
        app.netplay.phase,
        Phase::Preparing(Request::Host(..))
    ));
    assert_eq!(capture(&mut app), expected);
    app.pump_netplay();
    assert!(
        app.netplay.phase == Phase::Connecting,
        "{}",
        app.debug_windows.netplay.status
    );
    app.request_netplay_stop();
    assert!(app.netplay.phase == Phase::Idle);
    assert!(app.speed.paused);
    app.stop_emu_thread();
}

fn next_response(app: &App) -> EmuResponse {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match app.emu_thread.as_ref().unwrap().poll_response() {
            EmuResponsePoll::Response(response) => return *response,
            EmuResponsePoll::Disconnected => panic!("worker response disconnected"),
            EmuResponsePoll::Empty => {}
        }
        assert!(Instant::now() < deadline, "worker response deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
}
