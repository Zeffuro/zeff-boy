use super::*;
use crate::audio_discovery::{preview::PreviewRequest, render::RenderOptions};
use crate::emu_backend::loader::DirectGbaTasExecutionLoader;
use crate::tas_project::{
    TasAutosaveConfig, TasAutosaveStore, TasEditorSession, TasSeekStateCache,
};

fn app_snapshot(app: &crate::app::App) -> serde_json::Value {
    let session = app.debug_windows.tas_editor.active_session().unwrap();
    serde_json::json!({
        "core": zeff_firmware::sha256_hex(&state_bytes(app.emu_thread.as_ref().unwrap())),
        "tas_cursor": session.cursor(),
        "tas_content": format!("{:?}", session.project_content_sha256()),
        "tas_status": format!("{:?}", app.debug_windows.tas_editor.live_status()),
        "host_input": [app.host_input.buttons_pressed(), app.host_input.dpad_pressed()],
        "paused": app.speed.paused,
        "frames_in_flight": app.frames_in_flight,
        "debug": [app.debug_requests.step, app.debug_requests.next_frame, app.debug_requests.continue_, app.debug_requests.backstep],
        "audio_recorder": app.recording.audio_recorder.is_some(),
        "replay_recorder": app.recording.replay_recorder.is_some(),
        "replay_queue": app.recording.queued_replay_playback_frames,
        "replay_batches": app.recording.pending_replay_batches.len(),
    })
}

#[test]
fn audio_preview_preserves_core_tas_input_recording_and_battery_then_closes_cleanly() {
    let directory = crate::test_support::test_directory("app-audio-preview-isolation").unwrap();
    let rom_path = directory.path().join("game.gba");
    let mut rom = gba_fixture();
    rom[0xAC..0xB0].copy_from_slice(b"BPEE");
    rom.extend_from_slice(b"SRAM_V113");
    std::fs::write(&rom_path, rom).unwrap();
    let save_path = rom_path.with_extension("sav");
    let save = vec![0xA5; zeff_gba_core::hardware::constants::SRAM_SIZE];
    std::fs::write(&save_path, &save).unwrap();
    let loader = DirectGbaTasExecutionLoader::new(rom_path.clone());
    let mut project = loader.create_project().unwrap();
    project
        .edit_transaction(|edit| edit.insert_frames("main", 1, 2))
        .unwrap();
    let backend = loader.load_editor_engine(&project).unwrap().into_backend();
    assert!(matches!(&backend, EmuBackend::Gba(gba) if gba.tas_rtc_battery_bytes().is_some()));
    let manual = directory.path().join("movie.ztas");
    let autosaves =
        TasAutosaveStore::beside_manual_save(&manual, TasAutosaveConfig::default()).unwrap();
    let cache = TasSeekStateCache::open(directory.path().join("seek-cache")).unwrap();
    let mut session = TasEditorSession::new(project, manual, autosaves, cache).unwrap();
    session.set_cursor(1).unwrap();
    let mut app = app_with_worker(
        EmuThread::spawn(backend, false),
        1,
        ActiveSystem::GameBoyAdvance,
        rom_path,
    );
    app.debug_windows
        .tas_editor
        .install_verified_export_session(session);
    app.host_input
        .set_keyboard(crate::input::HostButton::A, true);
    app.set_user_paused(true);
    app.show_audio_explorer = true;
    app.refresh_audio_discovery();
    let source = app
        .debug_windows
        .audio_discovery
        .session
        .source
        .clone()
        .unwrap();
    let manifest = source.analyze(
        crate::audio_discovery::ScanLimits::default(),
        &std::sync::atomic::AtomicBool::new(false),
    );
    let request = PreviewRequest::prepare(&source, &manifest, 0, RenderOptions::default()).unwrap();
    let recorder_path = directory.path().join("game-recording.wav");
    app.recording.audio_recorder = Some(
        crate::audio_recorder::AudioRecorder::start(
            &recorder_path,
            48_000,
            crate::settings::AudioRecordingFormat::Wav16,
            None,
        )
        .unwrap(),
    );
    let before = app_snapshot(&app);
    let receiver = app
        .debug_windows
        .audio_discovery
        .preview_player()
        .start_captured(request);
    let mut callback = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut output = [0.0f32; 128];
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut nonzero = false;
    loop {
        app.refresh_audio_discovery();
        callback.fill(&mut output, 2);
        nonzero |= output.iter().any(|sample| *sample != 0.0);
        let player = app.debug_windows.audio_discovery.preview_player();
        assert!(player.error.is_none(), "{:?}", player.error);
        if player.snapshot().is_some_and(|state| state.position >= 512) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(nonzero);
    {
        let player = app.debug_windows.audio_discovery.preview_player();
        player.set_playing(false);
        player.seek(256);
        player.set_track_mask(1);
        player.seek(0);
        player.set_volume(25);
        player.set_playing(true);
    }
    assert_eq!(app_snapshot(&app), before);
    assert_eq!(std::fs::read(&save_path).unwrap(), save);
    app.emu_thread
        .as_ref()
        .unwrap()
        .send(EmuCommand::SetUncapped(true));
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut advanced = false;
    while !advanced
        || app
            .debug_windows
            .audio_discovery
            .preview_player()
            .snapshot()
            .unwrap()
            .position
            < 512
    {
        callback.fill(&mut output, 2);
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
    let before_close = app_snapshot(&app);
    app.show_audio_explorer = false;
    app.refresh_audio_discovery();
    callback.fill(&mut output, 2);
    assert!(output.iter().all(|sample| *sample == 0.0));
    assert!(
        app.debug_windows
            .audio_discovery
            .preview_player()
            .snapshot()
            .is_none()
    );
    assert_eq!(app_snapshot(&app), before_close);
    app.recording
        .audio_recorder
        .take()
        .unwrap()
        .finish()
        .unwrap();
    let recording = hound::WavReader::open(&recorder_path).unwrap();
    assert_eq!(
        recording.duration(),
        0,
        "preview must never enter the game recorder"
    );
    let request = PreviewRequest::prepare(&source, &manifest, 0, RenderOptions::default()).unwrap();
    let receiver = app
        .debug_windows
        .audio_discovery
        .preview_player()
        .start_captured(request);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut callback = loop {
        app.debug_windows.audio_discovery.preview_player().poll();
        if let Ok(callback) = receiver.try_recv() {
            break callback;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    app.stop_emu_thread();
    callback.fill(&mut output, 2);
    assert!(output.iter().all(|sample| *sample == 0.0));
    assert!(app.debug_windows.audio_discovery.session.source.is_none());
}
