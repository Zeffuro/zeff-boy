use super::super::{EmuLoop, terminal_battery_response};
use super::support::*;
use crate::emu_thread::contract_tests::{
    SMS_AUDIO_ROM, assert_active_audio_results_match, assert_gba_results_match, gba_rtc,
    gba_sram_bytes,
};
use crate::emu_thread::recovery::RecoveryTestConfig;
use crate::emu_thread::{EmuCommand, EmuResponse};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn terminal_battery_response_never_reports_a_failed_barrier_as_flushed() {
    let record = crate::save_paths::recovery_state::BatteryGenerationRecord {
        generation: 2,
        component_sha256: [3; 32],
    };
    assert!(matches!(
        terminal_battery_response(&Ok((Some("game.sav".to_string()), record))),
        EmuResponse::SramFlushed(Some(path)) if path == "game.sav"
    ));
    assert!(matches!(
        terminal_battery_response(&Err(anyhow::anyhow!("injected failure"))),
        EmuResponse::SramFlushFailed(error) if error == "injected failure"
    ));
}

fn terminal_test_loop(
    root: &crate::test_support::TestDirectory,
    fail_generation_write: bool,
) -> (
    EmuLoop,
    crossbeam_channel::Receiver<EmuResponse>,
    PathBuf,
    PathBuf,
) {
    let generation_path = root.path().join("battery-generation.bin");
    let state_path = root.path().join("last.smsstate");
    let (emu_loop, responses) = sega8_test_loop_with_recovery(
        SMS_AUDIO_ROM,
        root.path().join("fixture.sms"),
        true,
        Some(RecoveryTestConfig {
            generation_path: generation_path.clone(),
            state_path: state_path.clone(),
            fail_generation_write,
        }),
        true,
    );
    assert!(!emu_loop.backend.save_ram_kind().is_battery_backed());
    (emu_loop, responses, generation_path, state_path)
}

fn assert_terminal_success_responses(
    responses: &crossbeam_channel::Receiver<EmuResponse>,
    state_path: &std::path::Path,
) {
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::SramFlushed(None)
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::RecoverySaved(path) if path.as_path() == state_path
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::ShutdownComplete
    ));
    assert!(matches!(
        responses.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));
}

fn assert_no_sav_files(root: &std::path::Path) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                assert_ne!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("sav")
                );
            }
        }
    }
}

fn decode_terminal_files(
    emu_loop: &EmuLoop,
    generation_path: &std::path::Path,
    state_path: &std::path::Path,
) -> (
    Vec<u8>,
    Vec<u8>,
    crate::save_paths::recovery_state::BatteryGenerationRecord,
    crate::save_paths::recovery_state::RecoveryStateEnvelope,
) {
    let generation_bytes = std::fs::read(generation_path).unwrap();
    let state_bytes = std::fs::read(state_path).unwrap();
    let media_sha256 = emu_loop.backend.rom_hash();
    let record = crate::save_paths::recovery_state::decode_battery_generation(
        &generation_bytes,
        media_sha256,
    )
    .unwrap();
    let discriminator = emu_loop.backend.recovery_discriminator();
    let envelope = crate::save_paths::recovery_state::decode_recovery_state(
        &state_bytes,
        crate::save_paths::recovery_state::RecoveryStateIdentity {
            system: emu_loop.backend.system().storage_subdir(),
            discriminator: &discriminator,
            media_sha256,
        },
    )
    .unwrap();
    assert_eq!(
        record.component_sha256,
        emu_loop.backend.battery_component_hash()
    );
    assert_eq!(record.generation, 0);
    assert_eq!(
        record.component_sha256,
        crate::save_paths::recovery_state::canonical_battery_component_hash(&[])
    );
    assert_eq!(envelope.system, emu_loop.backend.system().storage_subdir());
    assert_eq!(envelope.discriminator, discriminator);
    assert_eq!(envelope.media_sha256, media_sha256);
    assert_eq!(
        envelope.battery,
        crate::save_paths::recovery_state::BatteryGenerationWitness::Committed {
            generation: record.generation,
            component_sha256: record.component_sha256,
        }
    );
    (generation_bytes, state_bytes, record, envelope)
}

#[test]
fn sega8_detached_terminal_recovery_matches_control_without_battery_files() {
    let control_root = crate::test_support::test_directory("sms-terminal-control").unwrap();
    let subject_root = crate::test_support::test_directory("sms-terminal-subject").unwrap();
    let (mut control, control_responses, control_generation, control_state) =
        terminal_test_loop(&control_root, false);
    let (mut subject, subject_responses, subject_generation, subject_state) =
        terminal_test_loop(&subject_root, false);
    subject.speculation.force_frames_for_test(1);

    assert!(control.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    assert!(subject.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    let control_result = control.drain_rx.recv().unwrap();
    let subject_result = subject.drain_rx.recv().unwrap();
    assert_active_audio_results_match(&control_result, &subject_result);
    let control_payload = control.backend.encode_state_bytes().unwrap();
    let subject_payload = subject.backend.encode_state_bytes().unwrap();
    assert_eq!(control_payload, subject_payload);
    assert_eq!(control.speculation.completed_runs_for_test(), 0);
    assert_eq!(subject.speculation.completed_runs_for_test(), 1);

    assert!(!control.handle_command(EmuCommand::Shutdown));
    assert!(!subject.handle_command(EmuCommand::Shutdown));
    assert_terminal_success_responses(&control_responses, &control_state);
    assert_terminal_success_responses(&subject_responses, &subject_state);
    assert_eq!(control.speculation.completed_runs_for_test(), 0);
    assert_eq!(subject.speculation.completed_runs_for_test(), 1);

    let (control_generation_bytes, control_state_bytes, control_record, control_envelope) =
        decode_terminal_files(&control, &control_generation, &control_state);
    let (subject_generation_bytes, subject_state_bytes, subject_record, subject_envelope) =
        decode_terminal_files(&subject, &subject_generation, &subject_state);
    assert_eq!(control_generation_bytes, subject_generation_bytes);
    assert_eq!(control_state_bytes, subject_state_bytes);
    assert_eq!(control_record, subject_record);
    assert_eq!(control_envelope, subject_envelope);
    assert_eq!(control_envelope.native_payload, control_payload);
    assert_eq!(subject_envelope.native_payload, subject_payload);
    assert_no_sav_files(control_root.path());
    assert_no_sav_files(subject_root.path());
}

fn gba_terminal_test_loop(
    root: &crate::test_support::TestDirectory,
) -> (
    EmuLoop,
    crossbeam_channel::Receiver<EmuResponse>,
    PathBuf,
    PathBuf,
    PathBuf,
) {
    let generation_path = root.path().join("battery-generation.bin");
    let state_path = root.path().join("last.gbastate");
    let rom_path = root.path().join("fixture.gba");
    let sav_path = rom_path.with_extension("sav");
    let (emu_loop, responses) = gba_test_loop_with_recovery(
        rom_path,
        true,
        Some(RecoveryTestConfig {
            generation_path: generation_path.clone(),
            state_path: state_path.clone(),
            fail_generation_write: false,
        }),
    );
    assert!(emu_loop.backend.save_ram_kind().is_battery_backed());
    (emu_loop, responses, sav_path, generation_path, state_path)
}

fn assert_gba_terminal_success_responses(
    responses: &crossbeam_channel::Receiver<EmuResponse>,
    sav_path: &std::path::Path,
    state_path: &std::path::Path,
) {
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::SramFlushed(Some(path)) if path == sav_path.display().to_string()
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::RecoverySaved(path) if path.as_path() == state_path
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::ShutdownComplete
    ));
    assert!(matches!(
        responses.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));
}

#[test]
fn gba_detached_terminal_sram_and_recovery_match_control() {
    let control_root = crate::test_support::test_directory("gba-terminal-control").unwrap();
    let subject_root = crate::test_support::test_directory("gba-terminal-subject").unwrap();
    let (mut control, control_responses, control_sav, control_generation, control_state) =
        gba_terminal_test_loop(&control_root);
    let (mut subject, subject_responses, subject_sav, subject_generation, subject_state) =
        gba_terminal_test_loop(&subject_root);
    subject.speculation.force_frames_for_test(1);

    assert!(control.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    assert!(subject.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    let control_result = control.drain_rx.recv().unwrap();
    let subject_result = subject.drain_rx.recv().unwrap();
    assert_gba_results_match(&control_result, &subject_result);
    let control_payload = control.backend.encode_state_bytes().unwrap();
    let subject_payload = subject.backend.encode_state_bytes().unwrap();
    assert_eq!(control_payload, subject_payload);
    assert_eq!(control.speculation.completed_runs_for_test(), 0);
    assert_eq!(subject.speculation.completed_runs_for_test(), 1);

    assert!(!control.handle_command(EmuCommand::Shutdown));
    assert!(!subject.handle_command(EmuCommand::Shutdown));
    assert_gba_terminal_success_responses(&control_responses, &control_sav, &control_state);
    assert_gba_terminal_success_responses(&subject_responses, &subject_sav, &subject_state);
    assert_eq!(control.speculation.completed_runs_for_test(), 0);
    assert_eq!(subject.speculation.completed_runs_for_test(), 1);

    let control_sram = std::fs::read(&control_sav).unwrap();
    let subject_sram = std::fs::read(&subject_sav).unwrap();
    assert_eq!(control_sram, subject_sram);
    assert_eq!(control_sram, gba_sram_bytes(control_sram.len()));
    assert_eq!(control_sram, gba_battery_and_rtc(&control.backend).0);
    assert_eq!(subject_sram, gba_battery_and_rtc(&subject.backend).0);
    assert_eq!(gba_battery_and_rtc(&control.backend).1, Some(gba_rtc()));
    assert_eq!(gba_battery_and_rtc(&subject.backend).1, Some(gba_rtc()));

    let control_generation_bytes = std::fs::read(&control_generation).unwrap();
    let subject_generation_bytes = std::fs::read(&subject_generation).unwrap();
    let control_state_bytes = std::fs::read(&control_state).unwrap();
    let subject_state_bytes = std::fs::read(&subject_state).unwrap();
    assert_eq!(control_generation_bytes, subject_generation_bytes);
    assert_eq!(control_state_bytes, subject_state_bytes);
    let media_sha256 = control.backend.rom_hash();
    let record = crate::save_paths::recovery_state::decode_battery_generation(
        &control_generation_bytes,
        media_sha256,
    )
    .unwrap();
    assert_eq!(
        record.component_sha256,
        control.backend.battery_component_hash()
    );
    let discriminator = control.backend.recovery_discriminator();
    let envelope = crate::save_paths::recovery_state::decode_recovery_state(
        &control_state_bytes,
        crate::save_paths::recovery_state::RecoveryStateIdentity {
            system: control.backend.system().storage_subdir(),
            discriminator: &discriminator,
            media_sha256,
        },
    )
    .unwrap();
    assert_eq!(
        envelope.battery,
        crate::save_paths::recovery_state::BatteryGenerationWitness::Committed {
            generation: record.generation,
            component_sha256: record.component_sha256,
        }
    );
    assert_eq!(envelope.native_payload, control_payload);
    assert_eq!(envelope.native_payload, subject_payload);
}

#[test]
fn sega8_detached_terminal_generation_failure_preserves_prior_recovery_envelope() {
    let root = crate::test_support::test_directory("sms-terminal-generation-failure").unwrap();
    let (mut emu_loop, responses, generation_path, state_path) = terminal_test_loop(&root, true);
    let media_sha256 = emu_loop.backend.rom_hash();
    let discriminator = emu_loop.backend.recovery_discriminator();
    let prior_envelope = crate::save_paths::recovery_state::RecoveryStateEnvelope {
        system: emu_loop.backend.system().storage_subdir().to_owned(),
        discriminator: discriminator.clone(),
        media_sha256,
        battery: crate::save_paths::recovery_state::BatteryGenerationWitness::Unknown,
        native_payload: emu_loop.backend.encode_state_bytes().unwrap(),
    };
    let prior_bytes =
        crate::save_paths::recovery_state::encode_recovery_state(&prior_envelope).unwrap();
    std::fs::write(&state_path, &prior_bytes).unwrap();
    emu_loop.speculation.force_frames_for_test(1);

    assert!(emu_loop.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    emu_loop.drain_rx.recv().unwrap();
    assert_eq!(emu_loop.speculation.completed_runs_for_test(), 1);
    assert!(!emu_loop.handle_command(EmuCommand::Shutdown));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::SramFlushFailed(error)
            if error == "injected battery generation write failure"
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::RecoverySaveFailed(error)
            if error == "injected battery generation write failure"
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::ShutdownComplete
    ));
    assert!(matches!(
        responses.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));
    assert_eq!(emu_loop.speculation.completed_runs_for_test(), 1);
    assert!(!generation_path.exists());
    assert_eq!(std::fs::read(&state_path).unwrap(), prior_bytes);
    let decoded = crate::save_paths::recovery_state::decode_recovery_state(
        &prior_bytes,
        crate::save_paths::recovery_state::RecoveryStateIdentity {
            system: emu_loop.backend.system().storage_subdir(),
            discriminator: &discriminator,
            media_sha256,
        },
    )
    .unwrap();
    assert_eq!(decoded, prior_envelope);
    assert_no_sav_files(root.path());
}

#[test]
fn paused_worker_wait_tracks_the_dirty_save_deadline() {
    let (mut emu_loop, _responses) = test_loop();
    let start = Instant::now();
    let interval = crate::emu_thread::persistence::BATTERY_FLUSH_INTERVAL;
    emu_loop.battery_flush = crate::emu_thread::persistence::BatteryFlushSchedule::new(start);

    assert_eq!(emu_loop.command_wait_timeout(start), None);
    emu_loop.battery_flush.mark_potentially_dirty();
    assert_eq!(emu_loop.command_wait_timeout(start), Some(interval));
    assert_eq!(
        emu_loop.command_wait_timeout(start + interval),
        Some(Duration::ZERO)
    );
}

#[test]
fn due_no_data_flush_clears_the_dirty_schedule() {
    let (mut emu_loop, _responses) = test_loop();
    let start = Instant::now();
    let deadline = start + crate::emu_thread::persistence::BATTERY_FLUSH_INTERVAL;
    emu_loop.battery_flush = crate::emu_thread::persistence::BatteryFlushSchedule::new(start);
    emu_loop.battery_flush.mark_potentially_dirty();

    emu_loop.flush_battery_sram_if_due(deadline);

    assert_eq!(emu_loop.command_wait_timeout(deadline), None);
}

#[test]
fn live_tcp_link_defers_periodic_flush_until_disconnect() {
    let (mut emu_loop, _responses, _peer) = test_gb_loop_with_tcp();
    let start = Instant::now();
    let now = start + crate::emu_thread::persistence::BATTERY_FLUSH_INTERVAL;
    emu_loop.battery_flush = crate::emu_thread::persistence::BatteryFlushSchedule::new(start);
    emu_loop.battery_flush.mark_potentially_dirty();

    assert_eq!(emu_loop.command_wait_timeout(now), None);
    emu_loop.disconnect_tcp_link();
    assert_eq!(emu_loop.command_wait_timeout(now), Some(Duration::ZERO));
}
