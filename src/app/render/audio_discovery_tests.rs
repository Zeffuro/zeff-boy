use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::tas_control::tests::harness::app_with_worker;
use crate::audio_discovery::test_support::gba_fixture;
use crate::debug::DebugUiActions;
use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::emu_thread::{EmuCommand, EmuResponse, EmuResponsePoll, EmuThread};

mod preview;

fn worker(path: &std::path::Path) -> EmuThread {
    let emu = zeff_gba_core::emulator::Emulator::from_rom_data(&gba_fixture()).unwrap();
    EmuThread::spawn(EmuBackend::from_gba(emu, path.to_owned()), false)
}

fn state_bytes(worker: &EmuThread) -> Vec<u8> {
    assert!(worker.send_checked(EmuCommand::CaptureStateBytes));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match worker.poll_response() {
            EmuResponsePoll::Response(response) => match *response {
                EmuResponse::StateCaptured(bytes) => return bytes,
                _ => panic!("unexpected response to state capture"),
            },
            EmuResponsePoll::Empty => {}
            EmuResponsePoll::Disconnected => panic!("emulator worker disconnected"),
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}

#[test]
fn audio_discovery_app_shares_worker_media_preserves_state_and_clears_on_stop_reload() {
    let directory = crate::test_support::test_directory("app-audio-discovery").unwrap();
    let path = directory.path().join("memory-only.gba");
    let mut app = app_with_worker(worker(&path), 1, ActiveSystem::GameBoyAdvance, path.clone());
    let source = app
        .emu_thread
        .as_ref()
        .unwrap()
        .audio_discovery_input()
        .unwrap();
    let same_source = app
        .emu_thread
        .as_ref()
        .unwrap()
        .audio_discovery_input()
        .unwrap();
    assert!(Arc::ptr_eq(&source, &same_source));
    assert!(!path.exists());
    let before = state_bytes(app.emu_thread.as_ref().unwrap());
    assert!(!app.render_frame(None));
    assert!(Arc::ptr_eq(
        app.debug_windows
            .audio_discovery
            .session
            .source
            .as_ref()
            .unwrap(),
        &source
    ));
    app.debug_windows.audio_discovery.session.start();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.debug_windows.audio_discovery.session.is_busy() {
        app.refresh_audio_discovery();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        app.debug_windows
            .audio_discovery
            .session
            .manifest
            .as_ref()
            .unwrap()
            .scan
            .candidates
            .len(),
        1
    );
    assert_eq!(state_bytes(app.emu_thread.as_ref().unwrap()), before);

    app.emu_thread
        .as_ref()
        .unwrap()
        .send(EmuCommand::SetUncapped(true));
    app.debug_windows.audio_discovery.session.start();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut advanced = false;
    while app.debug_windows.audio_discovery.session.is_busy() || !advanced {
        app.refresh_audio_discovery();
        if let Some(frame) = app.emu_thread.as_ref().unwrap().try_recv_frame() {
            assert!(frame.runtime_fault.is_none());
            advanced |= frame.advanced_frames > 0;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    app.emu_thread
        .as_ref()
        .unwrap()
        .send(EmuCommand::SetUncapped(false));
    assert_eq!(
        app.debug_windows
            .audio_discovery
            .session
            .manifest
            .as_ref()
            .unwrap()
            .scan
            .media
            .sha256
            .as_deref(),
        Some(zeff_firmware::sha256_hex(&source.bytes).as_str())
    );
    app.stop_game();
    assert!(app.debug_windows.audio_discovery.session.source.is_none());
    assert!(app.debug_windows.audio_discovery.session.manifest.is_none());
    app.emu_thread = Some(worker(&path));
    assert!(!app.render_debugger_frame(None));
    assert!(!Arc::ptr_eq(
        app.debug_windows
            .audio_discovery
            .session
            .source
            .as_ref()
            .unwrap(),
        &source
    ));
    assert!(app.debug_windows.audio_discovery.session.can_start());
    app.stop_emu_thread();
}

#[test]
fn audio_discovery_clears_when_replaced_by_an_unsupported_core() {
    let path = PathBuf::from("audio-discovery-session.gba");
    let mut app = app_with_worker(worker(&path), 1, ActiveSystem::GameBoyAdvance, path);
    app.refresh_audio_discovery();
    app.debug_windows.audio_discovery.session.start();
    app.stop_emu_thread();
    let nes = zeff_nes_core::emulator::Emulator::new(
        &crate::test_support::build_nes_test_rom(),
        48_000.0,
    )
    .unwrap();
    app.emu_thread = Some(EmuThread::spawn(
        EmuBackend::from_nes(nes, PathBuf::from("audio.nes")),
        false,
    ));
    app.refresh_audio_discovery();
    assert!(app.debug_windows.audio_discovery.session.source.is_none());
    assert!(!app.debug_windows.audio_discovery.session.can_start());
    assert!(app.debug_windows.audio_discovery.session.manifest.is_none());
    app.stop_emu_thread();
}

#[test]
fn legacy_debug_audio_link_opens_the_single_audio_explorer_window_state() {
    let path = PathBuf::from("audio-explorer-session.gba");
    let mut app = app_with_worker(worker(&path), 1, ActiveSystem::GameBoyAdvance, path);
    let mut actions = DebugUiActions::none();
    actions.open_audio_explorer = true;

    app.merge_debug_actions(actions);

    assert!(app.show_audio_explorer);
    assert!(app.focus_audio_explorer_pending);
    app.stop_emu_thread();
}
