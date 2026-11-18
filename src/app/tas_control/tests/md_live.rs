use super::harness::{app_with_worker, live_ok};
use super::*;
use crate::app::App;
use crate::emu_backend::{
    ActiveSystem,
    loader::{DirectMdTasExecutionLoader, MdTasProfile, md_product},
};
use crate::emu_thread::EmuThread;
use crate::live_control::LiveCommand;
use crate::tas_project::TasDigest;
use std::time::{Duration, Instant};

fn open(
    pad: MdTasProfile,
) -> (
    crate::test_support::TestDirectory,
    App,
    DirectMdTasExecutionLoader,
    Vec<u8>,
) {
    let directory = crate::test_support::test_directory("md-live-app").unwrap();
    let source = directory.path().join("game.md");
    std::fs::write(&source, crate::netplay::md_fixture::rom()).unwrap();
    let loader = DirectMdTasExecutionLoader::new(source.clone(), pad);
    let mut project = loader.create_project().unwrap();
    project
        .edit_transaction(|edit| {
            edit.insert_frames("main", 1, 3)?;
            let (one, two) = crate::netplay::md_fixture::words(pad.kind());
            for index in 0..4 {
                let mut input = crate::tas_project::TasInputFrame::default();
                input.players[0] = crate::tas_project::TasControllerInput {
                    buttons: one[index] as u8,
                    dpad: (one[index] >> 8) as u8,
                };
                input.players[1] = crate::tas_project::TasControllerInput {
                    buttons: two[index] as u8,
                    dpad: (two[index] >> 8) as u8,
                };
                edit.set_input_range("main", index as u64, 1, input)?;
            }
            Ok(())
        })
        .unwrap();
    let path = directory.path().join("movie.ztas");
    project.save_atomic(&path).unwrap();
    let mut backend = loader.load_fresh_backend().unwrap();
    backend.step_frame();
    backend.step_frame();
    let original = backend.encode_state_bytes().unwrap();
    let metadata = backend.replay_metadata();
    let resolved = md_product::loaded_md_tas_profile(&backend).unwrap();
    let mut app = app_with_worker(
        EmuThread::spawn(backend, false),
        405,
        ActiveSystem::Md,
        source,
    );
    app.rom_info.md_tas_profile = Some(resolved);
    app.rom_info.replay_metadata = Some(metadata);
    live_ok(&mut app, LiveCommand::TasOpenProject { path });
    settle(&mut app, |app| {
        app.tas_control_readiness_report().is_some_and(|report| {
            report.status == crate::app::tas_control::readiness::TasReadinessStatus::Ready
        })
    });
    (directory, app, loader, original)
}

fn step_control(
    control: &mut crate::emu_backend::EmuBackend,
    pad: MdTasProfile,
    index: usize,
) -> Vec<f32> {
    let (one, two) = crate::netplay::md_fixture::words(pad.kind());
    control.set_input(one[index] as u8, (one[index] >> 8) as u8);
    control.set_input_p2(two[index] as u8, (two[index] >> 8) as u8);
    control.step_frame();
    let mut pcm = Vec::new();
    control.drain_audio_samples_into(&mut pcm);
    assert!(pcm.iter().any(|value| *value != 0.0));
    let ram = crate::netplay::md_fixture::md(control).core().work_ram();
    assert_eq!(
        &ram[0x100..0x104],
        &crate::netplay::md_fixture::PINS[index % 2]
    );
    if pad == MdTasProfile::SixButton {
        assert_eq!(
            &ram[0x104..0x108],
            &crate::netplay::md_fixture::EXTRA_PINS[index % 2]
        );
    }
    pcm
}

fn settle(app: &mut App, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready(app) && Instant::now() < deadline {
        app.begin_queued_tas_control_acquire();
        app.drain_emu_responses();
        app.refresh_tas_editor_live_status();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(ready(app), "MD TAS state: {:?}", app.tas_control.state);
}

fn at(app: &App, cursor: u64) -> bool {
    matches!(app.tas_control.state, TasControlState::AwaitingDecision {
        candidate_executed_project_frames, .. } if candidate_executed_project_frames == cursor)
}

fn capture(app: &App) -> Vec<u8> {
    let worker = app.emu_thread.as_ref().unwrap();
    assert!(worker.send_checked(EmuCommand::CaptureStateBytes));
    match worker.recv_checked().unwrap() {
        EmuResponse::StateCaptured(bytes) => bytes,
        _ => panic!("state"),
    }
}

#[test]
fn native_md_app_spawned_worker_preview_seek_reseek_commit_and_rollback_preserve_project() {
    for pad in [MdTasProfile::ThreeButton, MdTasProfile::SixButton] {
        for keep in [false, true] {
            let (_directory, mut app, loader, original) = open(pad);
            let project_before = app
                .debug_windows
                .tas_editor
                .active_session()
                .unwrap()
                .project()
                .encode()
                .unwrap();
            live_ok(&mut app, LiveCommand::TasSelectBoundary { boundary: 3 });
            live_ok(
                &mut app,
                LiveCommand::TasLink {
                    at_end: false,
                    record: false,
                },
            );
            settle(&mut app, |app| at(app, 3));
            assert!(!app.worker_gameplay_commands_allowed() && app.speed.paused);
            for cursor in [1, 3, 1, 3] {
                live_ok(
                    &mut app,
                    LiveCommand::TasSelectBoundary { boundary: cursor },
                );
                live_ok(&mut app, LiveCommand::TasGoToSelection);
                settle(&mut app, |app| at(app, cursor));
            }
            live_ok(&mut app, LiveCommand::TasSelectBoundary { boundary: 1 });
            live_ok(&mut app, LiveCommand::TasGoToSelection);
            settle(&mut app, |app| at(app, 1));
            let mut control = loader.load_fresh_backend().unwrap();
            step_control(&mut control, pad, 0);
            live_ok(&mut app, LiveCommand::TasSetPlayback { active: true });
            for cursor in 2..=3 {
                let deadline = Instant::now() + Duration::from_secs(5);
                while !matches!(
                    app.tas_control.state,
                    TasControlState::PlaybackPending { .. }
                ) && Instant::now() < deadline
                {
                    app.pump_linked_tas_playback();
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert!(matches!(
                    app.tas_control.state,
                    TasControlState::PlaybackPending { .. }
                ));
                let pcm = step_control(&mut control, pad, cursor as usize - 1);
                let response = app.emu_thread.as_ref().unwrap().recv_checked().unwrap();
                match &response {
                    EmuResponse::TasFrameAdvanced {
                        executed_project_frames,
                        state_sha256,
                        audio_samples,
                        ..
                    } => {
                        assert_eq!(*executed_project_frames, cursor);
                        assert_eq!(
                            *state_sha256,
                            TasDigest::from_bytes(&control.encode_state_bytes().unwrap())
                        );
                        assert!(audio_samples.iter().any(|value| *value != 0.0));
                        assert_eq!(audio_samples, &pcm);
                        assert_eq!(
                            crate::netplay::md_fixture::bits(audio_samples),
                            crate::netplay::md_fixture::bits(&pcm)
                        );
                    }
                    _ => panic!("playback advance"),
                }
                assert!(app.consume_tas_control_response(response).is_none());
                let frame = app
                    .emu_thread
                    .as_ref()
                    .unwrap()
                    .shared_framebuffer()
                    .load_full()
                    .unwrap();
                assert_eq!(frame.as_slice(), control.display_framebuffer().0);
            }
            live_ok(&mut app, LiveCommand::TasSetPlayback { active: false });
            live_ok(&mut app, LiveCommand::TasDisconnect { keep });
            settle(&mut app, |app| {
                app.tas_control.state == TasControlState::Detached
            });
            let expected = if keep {
                let mut expected = loader.load_fresh_backend().unwrap();
                for index in 0..3 {
                    step_control(&mut expected, pad, index);
                }
                expected.encode_state_bytes().unwrap()
            } else {
                original
            };
            assert_eq!(capture(&app), expected);
            assert_eq!(
                app.debug_windows
                    .tas_editor
                    .active_session()
                    .unwrap()
                    .project()
                    .encode()
                    .unwrap(),
                project_before
            );
        }
    }
}

#[test]
fn native_md_fenced_acquisition_response_keeps_actual_app_paused_until_real_worker_return() {
    let (_directory, mut app, _loader, original) = open(MdTasProfile::ThreeButton);
    live_ok(
        &mut app,
        LiveCommand::TasLink {
            at_end: false,
            record: false,
        },
    );
    app.begin_queued_tas_control_acquire();
    let EmuResponse::TasControlAcquired {
        request_id,
        lease_id,
        witness,
    } = app.emu_thread.as_ref().unwrap().recv_checked().unwrap()
    else {
        panic!("acquire")
    };
    assert!(
        app.consume_tas_control_response(EmuResponse::TasControlAcquireFenced {
            request_id,
            lease_id,
            witness,
            error: "failed begin original verification".to_owned(),
        })
        .is_none()
    );
    assert!(matches!(
        app.tas_control.state,
        TasControlState::RollbackPending { .. }
    ));
    assert!(!app.worker_gameplay_commands_allowed() && app.speed.paused);
    settle(&mut app, |app| {
        app.tas_control.state == TasControlState::Detached
    });
    assert_eq!(capture(&app), original);
}
