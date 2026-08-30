use super::super::EmuLoop;
use super::support::*;
use crate::emu_thread::contract_tests::{
    assert_active_audio_results_match, assert_gba_results_match, gba_rtc, gba_sram_bytes,
};
use crate::emu_thread::{EmuCommand, EmuResponse, SpeculationBlockers, TcpLinkMode};
use std::time::{Duration, Instant};

#[test]
fn sega8_detached_stepframes_preserves_primary_and_host_results() {
    let (control, _control_responses) = audio_test_loop();
    let (subject, _subject_responses) = audio_test_loop();
    assert_sega8_detached_stepframes_match(control, subject);
}

fn assert_sega8_detached_stepframes_match(mut control: EmuLoop, mut subject: EmuLoop) {
    subject.speculation.force_frames_for_test(1);

    assert!(control.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    assert!(subject.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    assert_eq!(control.speculation.committed_frames_for_test(), 1);
    assert_eq!(subject.speculation.completed_runs_for_test(), 1);
    assert_eq!(subject.speculation.committed_frames_for_test(), 1);

    let control_result = control.drain_rx.recv().unwrap();
    let subject_result = subject.drain_rx.recv().unwrap();
    assert_active_audio_results_match(&control_result, &subject_result);

    assert_eq!(
        control.backend.encode_state_bytes().unwrap(),
        subject.backend.encode_state_bytes().unwrap()
    );
    assert_eq!(control.backend.framebuffer(), subject.backend.framebuffer());
    assert_eq!(
        control.backend.battery_component_hash(),
        subject.backend.battery_component_hash()
    );
    let after_dirty_deadline = Instant::now() + Duration::from_secs(60);
    assert_eq!(
        control.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );
    assert_eq!(
        subject.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );

    let mut control_audio = Vec::new();
    let mut subject_audio = Vec::new();
    control.backend.drain_audio_samples_into(&mut control_audio);
    subject.backend.drain_audio_samples_into(&mut subject_audio);
    assert_eq!(control_audio, subject_audio);
    assert!(control_audio.is_empty());

    let mut expected_detached = control
        .backend
        .fork_detached_for_speculation()
        .expect("control Sega8 backend should fork");
    expected_detached.disable_audio_output();
    assert!(expected_detached.step_frames(1));
    assert_eq!(
        subject.shared_framebuffer.load_full().unwrap().as_slice(),
        expected_detached.framebuffer()
    );
    assert_eq!(
        control.shared_framebuffer.load_full().unwrap().as_slice(),
        control.backend.framebuffer()
    );
}

#[test]
fn sega8_failed_tcp_link_start_leaves_detached_stepframes_eligible() {
    let (control, _control_responses) = audio_test_loop();
    let (mut subject, subject_responses) = audio_test_loop();

    assert!(
        subject.handle_command(EmuCommand::StartTcpLink(TcpLinkMode::Host {
            bind_addr: "127.0.0.1:0".to_string(),
        }))
    );
    assert!(matches!(
        subject_responses.try_recv(),
        Ok(EmuResponse::LinkFailed(error))
            if error == "TCP link currently supports GB/GBC and WonderSwan/WSC only"
    ));
    assert!(matches!(
        subject_responses.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));
    assert!(subject.pending_tcp_link.is_none());
    assert!(subject.tcp_link.is_none());
    assert!(subject.game_boy_replay_link.is_none());
    assert!(subject.wonder_swan_replay_link.is_none());
    assert!(!subject.periodic_battery_flush_blocked());

    assert_sega8_detached_stepframes_match(control, subject);
}

fn assert_sega8_detached_fallback(wrong_framebuffer_len: bool) {
    let (mut control, _control_responses) = audio_test_loop();
    let (mut subject, _subject_responses) = audio_test_loop();
    subject.speculation.force_frames_for_test(1);
    if wrong_framebuffer_len {
        subject.speculation.force_wrong_framebuffer_len_for_test();
    } else {
        subject.speculation.force_operational_failure_for_test();
    }

    assert!(control.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    assert!(subject.handle_command(EmuCommand::StepFrames(Box::new(active_audio_frame_input()))));
    assert_eq!(control.speculation.committed_frames_for_test(), 1);
    assert_eq!(subject.speculation.completed_runs_for_test(), 0);
    assert_eq!(subject.speculation.committed_frames_for_test(), 1);

    let control_result = control.drain_rx.recv().unwrap();
    let subject_result = subject.drain_rx.recv().unwrap();
    assert_active_audio_results_match(&control_result, &subject_result);
    assert_eq!(
        control.backend.encode_state_bytes().unwrap(),
        subject.backend.encode_state_bytes().unwrap()
    );
    assert_eq!(control.backend.framebuffer(), subject.backend.framebuffer());
    assert_eq!(
        control.backend.battery_component_hash(),
        subject.backend.battery_component_hash()
    );

    let after_dirty_deadline = Instant::now() + Duration::from_secs(60);
    assert_eq!(
        control.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );
    assert_eq!(
        subject.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );

    let mut control_audio = Vec::new();
    let mut subject_audio = Vec::new();
    control.backend.drain_audio_samples_into(&mut control_audio);
    subject.backend.drain_audio_samples_into(&mut subject_audio);
    assert_eq!(control_audio, subject_audio);
    assert!(control_audio.is_empty());
    assert_eq!(
        subject.shared_framebuffer.load_full().unwrap().as_slice(),
        subject.backend.framebuffer()
    );
}

#[test]
fn sega8_detached_operational_failure_preserves_primary_and_host_results() {
    assert_sega8_detached_fallback(false);
}

#[test]
fn sega8_detached_wrong_framebuffer_length_commits_the_primary_frame() {
    assert_sega8_detached_fallback(true);
}

#[test]
fn gba_detached_stepframes_preserves_primary_and_host_results() {
    let (mut control, _control_responses) = gba_test_loop();
    let (mut subject, _subject_responses) = gba_test_loop();
    subject.speculation.force_frames_for_test(1);

    assert!(control.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    assert!(subject.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    assert_eq!(control.speculation.committed_frames_for_test(), 1);
    assert_eq!(subject.speculation.completed_runs_for_test(), 1);
    assert_eq!(subject.speculation.committed_frames_for_test(), 1);

    let control_result = control.drain_rx.recv().unwrap();
    let subject_result = subject.drain_rx.recv().unwrap();
    assert_gba_results_match(&control_result, &subject_result);
    assert_eq!(
        control.backend.encode_state_bytes().unwrap(),
        subject.backend.encode_state_bytes().unwrap()
    );
    assert_eq!(control.backend.framebuffer(), subject.backend.framebuffer());
    let (control_sram, control_rtc) = gba_battery_and_rtc(&control.backend);
    let (subject_sram, subject_rtc) = gba_battery_and_rtc(&subject.backend);
    assert_eq!(control_sram, subject_sram);
    assert_eq!(control_sram, gba_sram_bytes(control_sram.len()));
    assert_eq!(control_rtc, subject_rtc);
    assert_eq!(control_rtc, Some(gba_rtc()));
    assert_eq!(subject_rtc, Some(gba_rtc()));

    let after_dirty_deadline = Instant::now() + Duration::from_secs(60);
    assert_eq!(
        control.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );
    assert_eq!(
        subject.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );
    let mut control_audio = Vec::new();
    let mut subject_audio = Vec::new();
    control.backend.drain_audio_samples_into(&mut control_audio);
    subject.backend.drain_audio_samples_into(&mut subject_audio);
    assert_eq!(control_audio, subject_audio);
    assert!(control_audio.is_empty());

    let mut expected_detached = control.backend.fork_detached_for_speculation().unwrap();
    expected_detached.disable_audio_output();
    assert!(expected_detached.step_frames(1));
    assert_eq!(
        subject.shared_framebuffer.load_full().unwrap().as_slice(),
        expected_detached.framebuffer()
    );
    assert_eq!(
        control.shared_framebuffer.load_full().unwrap().as_slice(),
        control.backend.framebuffer()
    );
}

fn assert_gba_detached_fallback(wrong_framebuffer_len: bool) {
    let (mut control, _control_responses) = gba_test_loop();
    let (mut subject, _subject_responses) = gba_test_loop();
    subject.speculation.force_frames_for_test(1);
    if wrong_framebuffer_len {
        subject.speculation.force_wrong_framebuffer_len_for_test();
    } else {
        subject.speculation.force_operational_failure_for_test();
    }

    assert!(control.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    assert!(subject.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    assert_eq!(control.speculation.committed_frames_for_test(), 1);
    assert_eq!(subject.speculation.completed_runs_for_test(), 0);
    assert_eq!(subject.speculation.committed_frames_for_test(), 1);
    let control_result = control.drain_rx.recv().unwrap();
    let subject_result = subject.drain_rx.recv().unwrap();
    assert_gba_results_match(&control_result, &subject_result);
    assert_eq!(
        control.backend.encode_state_bytes().unwrap(),
        subject.backend.encode_state_bytes().unwrap()
    );
    assert_eq!(control.backend.framebuffer(), subject.backend.framebuffer());
    let control_battery_and_rtc = gba_battery_and_rtc(&control.backend);
    let subject_battery_and_rtc = gba_battery_and_rtc(&subject.backend);
    assert_eq!(control_battery_and_rtc, subject_battery_and_rtc);
    assert_eq!(control_battery_and_rtc.1, Some(gba_rtc()));
    assert_eq!(subject_battery_and_rtc.1, Some(gba_rtc()));
    let after_dirty_deadline = Instant::now() + Duration::from_secs(60);
    assert_eq!(
        control.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );
    assert_eq!(
        subject.battery_flush.wait_timeout(after_dirty_deadline),
        Some(Duration::ZERO)
    );
    let mut control_audio = Vec::new();
    let mut subject_audio = Vec::new();
    control.backend.drain_audio_samples_into(&mut control_audio);
    subject.backend.drain_audio_samples_into(&mut subject_audio);
    assert_eq!(control_audio, subject_audio);
    assert!(control_audio.is_empty());
    assert_eq!(
        subject.shared_framebuffer.load_full().unwrap().as_slice(),
        subject.backend.framebuffer()
    );
}

#[test]
fn gba_detached_stepframes_falls_back_on_operational_or_size_failure() {
    assert_gba_detached_fallback(false);
    assert_gba_detached_fallback(true);
}

#[test]
fn sega8_detached_stepframes_rejects_nonlocal_or_mutating_requests() {
    let (mut replay_timeline, _responses) = test_loop();
    replay_timeline.speculation.force_frames_for_test(1);
    let mut replay_timeline_input = frame_input(1);
    replay_timeline_input.speculation_blockers =
        SpeculationBlockers::from_app_for_test(true, false);
    assert!(
        replay_timeline.handle_command(EmuCommand::StepFrames(Box::new(replay_timeline_input)))
    );
    assert_eq!(replay_timeline.speculation.completed_runs_for_test(), 0);

    let (mut live_control, _responses) = test_loop();
    live_control.speculation.force_frames_for_test(1);
    let mut live_control_input = frame_input(1);
    live_control_input.speculation_blockers = SpeculationBlockers::from_app_for_test(false, true);
    assert!(live_control.handle_command(EmuCommand::StepFrames(Box::new(live_control_input))));
    assert_eq!(live_control.speculation.completed_runs_for_test(), 0);

    let (mut replay, _responses) = test_loop();
    replay.speculation.force_frames_for_test(1);
    let mut replay_input = frame_input(1);
    replay_input.replay_joypad_frames = Some(vec![crate::emu_thread::ReplayJoypadFrame::default()]);
    assert!(replay.handle_command(EmuCommand::StepFrames(Box::new(replay_input))));
    assert_eq!(replay.speculation.completed_runs_for_test(), 0);

    let (mut debugger, _responses) = test_loop();
    debugger.speculation.force_frames_for_test(1);
    let mut debugger_input = frame_input(1);
    debugger_input
        .debug_actions
        .memory_writes
        .push((0xC000, 0x5A));
    assert!(debugger.handle_command(EmuCommand::StepFrames(Box::new(debugger_input))));
    assert_eq!(debugger.speculation.completed_runs_for_test(), 0);

    let (mut batch, _responses) = test_loop();
    batch.speculation.force_frames_for_test(1);
    assert!(batch.handle_command(EmuCommand::StepFrames(Box::new(frame_input(2)))));
    assert_eq!(batch.speculation.completed_runs_for_test(), 0);

    let (mut uncapped, _responses) = test_loop();
    uncapped.speculation.force_frames_for_test(1);
    uncapped.uncapped_mode = true;
    assert!(uncapped.handle_command(EmuCommand::StepFrames(Box::new(frame_input(1)))));
    assert_eq!(uncapped.speculation.completed_runs_for_test(), 0);
}

#[test]
fn gba_detached_stepframes_rejects_nonlocal_or_mutating_requests() {
    let (mut replay_timeline, _responses) = gba_test_loop();
    replay_timeline.speculation.force_frames_for_test(1);
    let mut replay_timeline_input = gba_frame_input();
    replay_timeline_input.speculation_blockers =
        SpeculationBlockers::from_app_for_test(true, false);
    assert!(
        replay_timeline.handle_command(EmuCommand::StepFrames(Box::new(replay_timeline_input)))
    );
    assert_eq!(replay_timeline.speculation.completed_runs_for_test(), 0);

    let (mut live_control, _responses) = gba_test_loop();
    live_control.speculation.force_frames_for_test(1);
    let mut live_control_input = gba_frame_input();
    live_control_input.speculation_blockers = SpeculationBlockers::from_app_for_test(false, true);
    assert!(live_control.handle_command(EmuCommand::StepFrames(Box::new(live_control_input))));
    assert_eq!(live_control.speculation.completed_runs_for_test(), 0);

    let (mut replay, _responses) = gba_test_loop();
    replay.speculation.force_frames_for_test(1);
    let mut replay_input = gba_frame_input();
    replay_input.replay_joypad_frames = Some(vec![crate::emu_thread::ReplayJoypadFrame::default()]);
    assert!(replay.handle_command(EmuCommand::StepFrames(Box::new(replay_input))));
    assert_eq!(replay.speculation.completed_runs_for_test(), 0);

    let (mut debugger, _responses) = gba_test_loop();
    debugger.speculation.force_frames_for_test(1);
    let mut debugger_input = gba_frame_input();
    debugger_input
        .debug_actions
        .memory_writes
        .push((0x0200_0000, 0x5A));
    assert!(debugger.handle_command(EmuCommand::StepFrames(Box::new(debugger_input))));
    assert_eq!(debugger.speculation.completed_runs_for_test(), 0);

    let (mut batch, _responses) = gba_test_loop();
    batch.speculation.force_frames_for_test(1);
    let mut batch_input = gba_frame_input();
    batch_input.frames = 2;
    assert!(batch.handle_command(EmuCommand::StepFrames(Box::new(batch_input))));
    assert_eq!(batch.speculation.completed_runs_for_test(), 0);

    let (mut uncapped, _responses) = gba_test_loop();
    uncapped.speculation.force_frames_for_test(1);
    uncapped.uncapped_mode = true;
    assert!(uncapped.handle_command(EmuCommand::StepFrames(Box::new(gba_frame_input()))));
    assert_eq!(uncapped.speculation.completed_runs_for_test(), 0);
}
