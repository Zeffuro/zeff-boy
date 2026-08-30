use super::support::{
    frame_input, tas_nes_test_backend, tas_nes_test_backend_from_rom, tas_nes_test_loop,
    tas_nes_test_loop_from_backend, tas_nes_test_loop_with_recovery, test_gb_loop_with_tcp,
    test_loop,
};
use crate::emu_thread::{
    AudioRecordingCapture, EmuCommand, EmuResponse, EmuThread, TasControlAcquireRejectedReason,
    TasControlCommandKind, TasControlCommitRejectedReason, TasControlLeaseWitness,
    TasControlRollbackRejectedReason, TasExecutionProfile,
};
use std::ops::ControlFlow;

use super::super::tas_control::{TasControl, TasControlContext, TasRestoredCheckpoint};

fn acquire(emu_loop: &mut super::super::EmuLoop, request_id: u64) {
    assert!(emu_loop.handle_command(EmuCommand::AcquireTasControl {
        request_id,
        profile: TasExecutionProfile::DirectNesCartridge,
    }));
}

#[test]
fn tas_control_lease_is_exclusive_non_mutating_and_token_checked() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let before = emu_loop.backend.encode_state_bytes().unwrap();

    acquire(&mut emu_loop, 40);
    let witness = match responses.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: 40,
            lease_id: 1,
            witness,
        } => witness,
        _ => panic!("unexpected TAS control response"),
    };
    assert_eq!(witness.frame_count, 0);
    assert_eq!(witness.current_state_bytes, before);
    assert_eq!(
        witness.current_state_sha256,
        crate::tas_project::TasDigest::from_bytes(&witness.current_state_bytes)
    );
    assert_eq!(witness.source_media_sha256, witness.effective_media_sha256);
    assert_eq!(
        witness.effective_media_sha256,
        crate::tas_project::TasDigest(emu_loop.backend.rom_hash())
    );
    assert_eq!(
        witness.determinism_abi,
        zeff_nes_core::save_state::TAS_DETERMINISM_ABI_ID
    );
    assert_eq!(
        witness.state_format_compatibility_id,
        zeff_nes_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID
    );
    assert_eq!(
        witness.sync_config_sha256,
        crate::emu_backend::loader::direct_nes_tas_sync_config_sha256()
    );
    assert_eq!(emu_loop.backend.frame_count(), 0);
    assert!(emu_loop.periodic_battery_flush_blocked());

    acquire(&mut emu_loop, 41);
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlAcquireRejected {
            request_id: 41,
            reason: TasControlAcquireRejectedReason::AlreadyLeased { lease_id: 1 },
        }
    ));

    assert!(emu_loop.handle_command(EmuCommand::StepFrames(Box::new(frame_input(1)))));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlCommandRejected {
            lease_id: 1,
            command: TasControlCommandKind::FrameExecution,
        }
    ));
    assert!(emu_loop.drain_rx.is_empty());
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), before);

    assert!(emu_loop.handle_command(EmuCommand::LoadStateBytes {
        state_bytes: before.clone(),
        buttons_pressed: 0,
        dpad_pressed: 0,
        replay_events: None,
        game_boy_link_start_state: None,
        game_boy_link_coordinator_start_state: None,
        game_boy_link_start_tick: None,
        wonder_swan_link_start_tick: None,
    }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlCommandRejected {
            lease_id: 1,
            command: TasControlCommandKind::StateOrRecovery,
        }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), before);

    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id: 2 }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRollbackRejected {
            requested_lease_id: 2,
            reason: TasControlRollbackRejectedReason::WrongLease { active_lease_id: 1 },
        }
    ));
    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id: 1 }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack {
            lease_id: 1,
            restored_state_sha256,
            frame_count: 0,
        } if restored_state_sha256 == crate::tas_project::TasDigest::from_bytes(&before)
    ));
    assert!(!emu_loop.periodic_battery_flush_blocked());
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), before);

    assert!(emu_loop.handle_command(EmuCommand::StepFrames(Box::new(frame_input(1)))));
    assert_eq!(emu_loop.drain_rx.recv().unwrap().advanced_frames, 1);
}

#[test]
fn rollback_restores_the_worker_checkpoint_bytes_and_frame() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    let checkpoint = emu_loop.backend.encode_state_bytes().unwrap();
    let checkpoint_framebuffer = emu_loop.backend.framebuffer().to_vec();
    assert!(emu_loop.handle_command(EmuCommand::StepFrames(Box::new(frame_input(1)))));
    assert_eq!(emu_loop.drain_rx.recv().unwrap().advanced_frames, 1);
    let candidate = emu_loop.backend.encode_state_bytes().unwrap();
    emu_loop
        .backend
        .load_state_from_bytes(checkpoint.clone())
        .unwrap();

    acquire(&mut emu_loop, 42);
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlAcquired {
            request_id: 42,
            lease_id: 1,
            ..
        }
    ));
    emu_loop
        .backend
        .load_state_from_bytes(candidate.clone())
        .unwrap();
    let candidate_framebuffer = vec![0xA5; checkpoint_framebuffer.len()];
    crate::emu_thread::types::publish_owned_framebuffer(
        &emu_loop.shared_framebuffer,
        candidate_framebuffer.clone(),
    );
    emu_loop
        .rewind_buffer
        .push(&candidate, &candidate_framebuffer);
    emu_loop
        .runtime_fault
        .latch(Some("candidate fault".to_owned()));
    emu_loop
        .pending_audio_discontinuities
        .push(crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation);
    assert_eq!(emu_loop.backend.frame_count(), 1);
    assert_eq!(emu_loop.rewind_buffer.len(), 1);

    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id: 1 }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack {
            lease_id: 1,
            restored_state_sha256,
            frame_count: 0,
        } if restored_state_sha256 == crate::tas_project::TasDigest::from_bytes(&checkpoint)
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
    assert_eq!(emu_loop.backend.framebuffer(), checkpoint_framebuffer);
    assert_eq!(
        emu_loop.shared_framebuffer.load_full().unwrap().as_slice(),
        checkpoint_framebuffer
    );
    assert!(emu_loop.rewind_buffer.is_empty());
    assert!(emu_loop.runtime_fault.can_step());
    assert!(emu_loop.pending_audio_discontinuities.is_empty());
    assert!(!emu_loop.tas_control.is_leased());

    assert!(emu_loop.handle_command(EmuCommand::Rewind(1)));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::RewindFailed(_)
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

#[test]
fn commit_before_execution_and_duplicate_tokens_are_inert() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    acquire(&mut emu_loop, 43);
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlAcquired { lease_id: 1, .. }
    ));
    let checkpoint = emu_loop.backend.encode_state_bytes().unwrap();

    assert!(emu_loop.handle_command(EmuCommand::CommitTasControl { lease_id: 2 }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlCommitRejected {
            requested_lease_id: 2,
            reason: TasControlCommitRejectedReason::WrongLease { active_lease_id: 1 },
        }
    ));
    assert!(emu_loop.tas_control.is_leased());
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);

    assert!(emu_loop.handle_command(EmuCommand::CommitTasControl { lease_id: 1 }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlCommitRejected {
            requested_lease_id: 1,
            reason: TasControlCommitRejectedReason::NoCompletedExecution,
        }
    ));
    assert!(emu_loop.tas_control.is_leased());
    assert!(emu_loop.handle_command(EmuCommand::RollbackTasControl { lease_id: 1 }));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlRolledBack { lease_id: 1, .. }
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);
}

#[test]
fn tas_control_acquisition_reports_each_worker_blocker() {
    let (mut uncapped, uncapped_responses) = test_loop();
    assert!(uncapped.handle_command(EmuCommand::SetUncapped(true)));
    acquire(&mut uncapped, 1);
    assert!(matches!(
        uncapped_responses.recv().unwrap(),
        EmuResponse::TasControlAcquireRejected {
            request_id: 1,
            reason: TasControlAcquireRejectedReason::UncappedExecution,
        }
    ));

    let (mut recording, recording_responses) = test_loop();
    assert!(
        recording.handle_command(EmuCommand::SetAudioRecordingCapture {
            capture: AudioRecordingCapture {
                active: true,
                semantic: false,
            },
            acknowledged: None,
        })
    );
    acquire(&mut recording, 2);
    assert!(matches!(
        recording_responses.recv().unwrap(),
        EmuResponse::TasControlAcquireRejected {
            request_id: 2,
            reason: TasControlAcquireRejectedReason::AudioRecordingActive,
        }
    ));

    let (mut pending_frame, pending_frame_responses) = tas_nes_test_loop();
    assert!(pending_frame.handle_command(EmuCommand::StepFrames(Box::new(frame_input(1)))));
    acquire(&mut pending_frame, 3);
    assert!(matches!(
        pending_frame_responses.recv().unwrap(),
        EmuResponse::TasControlAcquireRejected {
            request_id: 3,
            reason: TasControlAcquireRejectedReason::PendingFrameDelivery,
        }
    ));
    let _ = pending_frame.drain_rx.recv().unwrap();
    acquire(&mut pending_frame, 4);
    assert!(matches!(
        pending_frame_responses.recv().unwrap(),
        EmuResponse::TasControlAcquired {
            request_id: 4,
            lease_id: 1,
            witness,
        } if witness.frame_count == 1
    ));

    let (mut faulted, faulted_responses) = test_loop();
    faulted.runtime_fault.latch(Some("fault".to_owned()));
    acquire(&mut faulted, 5);
    assert!(matches!(
        faulted_responses.recv().unwrap(),
        EmuResponse::TasControlAcquireRejected {
            request_id: 5,
            reason: TasControlAcquireRejectedReason::RuntimeFault,
        }
    ));

    let (mut linked, linked_responses, peer) = test_gb_loop_with_tcp();
    acquire(&mut linked, 6);
    assert!(matches!(
        linked_responses.recv().unwrap(),
        EmuResponse::TasControlAcquireRejected {
            request_id: 6,
            reason: TasControlAcquireRejectedReason::LinkActivity,
        }
    ));
    drop(peer);
}

#[test]
fn worker_owned_cheat_state_refuses_acquisition() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    emu_loop
        .last_cheats
        .push(crate::cheats::CheatPatch::RamWrite {
            address: 0,
            value: crate::cheats::CheatValue::constant(1),
        });

    acquire(&mut emu_loop, 7);
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlAcquireRejected {
            request_id: 7,
            reason: TasControlAcquireRejectedReason::CheatsPresent,
        }
    ));
}

fn vs_mapper_rom(mapper_id: u8) -> Vec<u8> {
    let (prg_banks, chr_banks) = match mapper_id {
        99 => (2, 2),
        98 => (4, 8),
        151 => (4, 4),
        _ => unreachable!(),
    };
    let mut rom = vec![0; 16 + prg_banks * 0x4000 + chr_banks * 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = prg_banks as u8;
    rom[5] = chr_banks as u8;
    rom[6] = (mapper_id & 0x0F) << 4;
    rom[7] = mapper_id & 0xF0;
    if mapper_id == 99 {
        rom[7] |= 1;
    }
    let reset = 16 + prg_banks * 0x4000 - 4;
    rom[reset] = 0;
    rom[reset + 1] = 0x80;
    rom
}

#[test]
fn vs_hardware_mappers_refuse_the_ordinary_direct_nes_profile() {
    for mapper_id in [98, 99, 151] {
        let rom = vs_mapper_rom(mapper_id);
        let backend = tas_nes_test_backend_from_rom(
            &format!("tas-control-vs-mapper-{mapper_id}"),
            rom.clone(),
        );
        assert!(!backend.nes().unwrap().has_standard_console_hardware());
        let state = backend.encode_state_bytes().unwrap();
        assert!(
            crate::emu_backend::loader::direct_nes_tas_identity(&backend, &rom, &state).is_err()
        );
        let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);

        acquire(&mut emu_loop, u64::from(mapper_id));
        assert!(matches!(
            responses.recv().unwrap(),
            EmuResponse::TasControlAcquireRejected {
                reason: TasControlAcquireRejectedReason::NonStandardConsoleHardware,
                ..
            }
        ));
    }
}

#[test]
fn non_home_console_headers_refuse_the_ordinary_direct_nes_profile() {
    for (label, flags7, byte13) in [("playchoice", 0x02, 0), ("extended", 0x0B, 1)] {
        let mut rom = crate::test_support::build_nes_test_rom();
        rom[7] = flags7;
        rom[13] = byte13;
        let backend =
            tas_nes_test_backend_from_rom(&format!("tas-control-{label}-console"), rom.clone());
        assert!(!backend.nes().unwrap().has_standard_console_hardware());
        let state = backend.encode_state_bytes().unwrap();
        assert!(
            crate::emu_backend::loader::direct_nes_tas_identity(&backend, &rom, &state).is_err()
        );
        let (mut emu_loop, responses) = tas_nes_test_loop_from_backend(backend);

        acquire(&mut emu_loop, 160);
        assert!(matches!(
            responses.recv().unwrap(),
            EmuResponse::TasControlAcquireRejected {
                reason: TasControlAcquireRejectedReason::NonStandardConsoleHardware,
                ..
            }
        ));
    }
}

#[test]
fn tas_control_commands_observe_fifo_worker_order() {
    let backend = tas_nes_test_backend();
    let thread = EmuThread::spawn(backend, false);

    thread.send(EmuCommand::SetUncapped(true));
    thread.send(EmuCommand::AcquireTasControl {
        request_id: 10,
        profile: TasExecutionProfile::DirectNesCartridge,
    });
    assert!(matches!(
        thread.recv().unwrap(),
        EmuResponse::TasControlAcquireRejected {
            request_id: 10,
            reason: TasControlAcquireRejectedReason::UncappedExecution,
        }
    ));

    thread.send(EmuCommand::SetUncapped(false));
    thread.send(EmuCommand::AcquireTasControl {
        request_id: 11,
        profile: TasExecutionProfile::DirectNesCartridge,
    });
    let (lease_id, start_state) = match thread.recv().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: 11,
            lease_id,
            witness,
        } => (lease_id, witness.current_state_bytes),
        EmuResponse::TasControlAcquireRejected {
            request_id: 11,
            reason: TasControlAcquireRejectedReason::PendingFrameDelivery,
        } => {
            while thread.try_recv_frame().is_some() {}
            thread.send(EmuCommand::AcquireTasControl {
                request_id: 12,
                profile: TasExecutionProfile::DirectNesCartridge,
            });
            match thread.recv().unwrap() {
                EmuResponse::TasControlAcquired {
                    request_id: 12,
                    lease_id,
                    witness,
                } => (lease_id, witness.current_state_bytes),
                _ => panic!("unexpected TAS control response"),
            }
        }
        _ => panic!("unexpected TAS control response"),
    };
    assert_eq!(lease_id, 1);
    thread.send(EmuCommand::SetUncapped(true));
    assert!(matches!(
        thread.recv().unwrap(),
        EmuResponse::TasControlCommandRejected {
            lease_id,
            command: TasControlCommandKind::AudioOrTimingConfiguration,
        } if lease_id == 1
    ));
    thread.send(EmuCommand::ExecuteTasControl(Box::new(
        crate::emu_thread::TasExecutionRequest {
            profile: crate::emu_thread::TasExecutionProfile::DirectNesCartridge,
            lease_id,
            run_id: 1,
            start_state_bytes: start_state,
            input_prefix: vec![crate::emu_thread::TasInputFrame::default()],
        },
    )));
    assert!(matches!(
        thread.recv().unwrap(),
        EmuResponse::TasExecutionCompleted {
            lease_id: completed_lease,
            run_id: 1,
            ..
        } if completed_lease == lease_id
    ));
    thread.send(EmuCommand::CommitTasControl { lease_id });
    assert!(matches!(
        thread.recv().unwrap(),
        EmuResponse::TasControlCommitted { lease_id: committed } if committed == lease_id
    ));
}

#[test]
fn tas_control_shutdown_bypasses_the_lease_gate() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    acquire(&mut emu_loop, 20);
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlAcquired { lease_id: 1, .. }
    ));

    assert!(!emu_loop.handle_command(EmuCommand::Shutdown));
    let mut saw_shutdown = false;
    while let Ok(response) = responses.try_recv() {
        if matches!(response, EmuResponse::ShutdownComplete) {
            saw_shutdown = true;
        }
        assert!(!matches!(
            response,
            EmuResponse::TasControlCommandRejected { .. }
        ));
    }
    assert!(saw_shutdown);
}

#[test]
fn leased_shutdown_restores_checkpoint_before_recovery_persistence() {
    let root = crate::test_support::test_directory("tas-control-terminal-rollback").unwrap();
    let (mut emu_loop, responses, generation_path, state_path) =
        tas_nes_test_loop_with_recovery(&root);
    let checkpoint = emu_loop.backend.encode_state_bytes().unwrap();
    assert!(emu_loop.handle_command(EmuCommand::StepFrames(Box::new(frame_input(1)))));
    assert_eq!(emu_loop.drain_rx.recv().unwrap().advanced_frames, 1);
    let candidate = emu_loop.backend.encode_state_bytes().unwrap();
    emu_loop
        .backend
        .load_state_from_bytes(checkpoint.clone())
        .unwrap();
    acquire(&mut emu_loop, 21);
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlAcquired { lease_id: 1, .. }
    ));
    emu_loop.backend.load_state_from_bytes(candidate).unwrap();

    assert!(!emu_loop.handle_command(EmuCommand::Shutdown));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::SramFlushed(None)
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::RecoverySaved(path) if path == state_path
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::ShutdownComplete
    ));
    assert_eq!(emu_loop.backend.encode_state_bytes().unwrap(), checkpoint);

    let encoded = std::fs::read(&state_path).unwrap();
    let discriminator = emu_loop.backend.recovery_discriminator();
    let envelope = crate::save_paths::recovery_state::decode_recovery_state(
        &encoded,
        crate::save_paths::recovery_state::RecoveryStateIdentity {
            system: emu_loop.backend.system().storage_subdir(),
            discriminator: &discriminator,
            media_sha256: emu_loop.backend.rom_hash(),
        },
    )
    .unwrap();
    assert_eq!(envelope.native_payload, checkpoint);
    assert!(generation_path.exists());
}

#[test]
fn leased_shutdown_restore_failure_blocks_candidate_persistence() {
    let root = crate::test_support::test_directory("tas-control-terminal-failure").unwrap();
    let (mut emu_loop, responses, generation_path, state_path) =
        tas_nes_test_loop_with_recovery(&root);
    acquire(&mut emu_loop, 22);
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlAcquired { lease_id: 1, .. }
    ));
    emu_loop.tas_control.corrupt_checkpoint_for_test();

    assert!(!emu_loop.handle_command(EmuCommand::Shutdown));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::SramFlushFailed(error) if error.contains("TAS checkpoint restoration failed")
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::RecoverySaveFailed(error) if error.contains("TAS checkpoint restoration failed")
    ));
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::ShutdownComplete
    ));
    assert!(!generation_path.exists());
    assert!(!state_path.exists());
    assert!(emu_loop.tas_control.is_leased());
}

#[test]
fn tas_control_lease_ids_fail_closed_at_exhaustion() {
    let (mut emu_loop, responses) = tas_nes_test_loop();
    emu_loop.tas_control.set_next_lease_id(u64::MAX);

    acquire(&mut emu_loop, 30);
    assert!(matches!(
        responses.recv().unwrap(),
        EmuResponse::TasControlAcquireRejected {
            request_id: 30,
            reason: TasControlAcquireRejectedReason::LeaseIdExhausted,
        }
    ));
    assert!(!emu_loop.tas_control.is_leased());
}

fn unblocked_context() -> TasControlContext {
    TasControlContext {
        uncapped_execution: false,
        audio_recording_active: false,
        link_activity: false,
        pending_frame_delivery: false,
        runtime_fault: false,
    }
}

fn dummy_witness() -> TasControlLeaseWitness {
    TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectNesCartridge,
        frame_count: 0,
        source_media_sha256: crate::tas_project::TasDigest([1; 32]),
        effective_media_sha256: crate::tas_project::TasDigest([1; 32]),
        current_state_bytes: vec![2, 3],
        current_state_sha256: crate::tas_project::TasDigest::from_bytes(&[2, 3]),
        determinism_abi: zeff_nes_core::save_state::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id: zeff_nes_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: crate::emu_backend::loader::direct_nes_tas_sync_config_sha256(),
    }
}

#[test]
fn acquisition_failure_does_not_install_or_consume_a_lease() {
    let mut control = TasControl::new();
    assert!(matches!(
        control.dispatch(
            EmuCommand::AcquireTasControl {
                request_id: 80,
                profile: TasExecutionProfile::DirectNesCartridge,
            },
            unblocked_context(),
            |_| Err(TasControlAcquireRejectedReason::StateWitnessUnavailable),
        ),
        ControlFlow::Break(EmuResponse::TasControlAcquireRejected {
            request_id: 80,
            reason: TasControlAcquireRejectedReason::StateWitnessUnavailable,
        })
    ));
    assert!(!control.is_leased());
    let mut inconsistent = dummy_witness();
    inconsistent.current_state_sha256 = crate::tas_project::TasDigest([7; 32]);
    assert!(matches!(
        control.dispatch(
            EmuCommand::AcquireTasControl {
                request_id: 81,
                profile: TasExecutionProfile::DirectNesCartridge,
            },
            unblocked_context(),
            |_| Ok(inconsistent),
        ),
        ControlFlow::Break(EmuResponse::TasControlAcquireRejected {
            request_id: 81,
            reason: TasControlAcquireRejectedReason::StateWitnessUnavailable,
        })
    ));
    assert!(!control.is_leased());
    assert!(matches!(
        control.dispatch(
            EmuCommand::AcquireTasControl {
                request_id: 82,
                profile: TasExecutionProfile::DirectNesCartridge,
            },
            unblocked_context(),
            |_| Ok(dummy_witness()),
        ),
        ControlFlow::Break(EmuResponse::TasControlAcquired {
            request_id: 82,
            lease_id: 1,
            ..
        })
    ));
}

#[test]
fn rollback_restore_and_verification_failures_keep_the_exact_lease_fenced() {
    let mut control = TasControl::new();
    assert!(matches!(
        control.dispatch(
            EmuCommand::AcquireTasControl {
                request_id: 82,
                profile: TasExecutionProfile::DirectNesCartridge,
            },
            unblocked_context(),
            |_| Ok(dummy_witness()),
        ),
        ControlFlow::Break(EmuResponse::TasControlAcquired { lease_id: 1, .. })
    ));

    assert!(matches!(
        control.rollback(1, |_| Err(TasControlRollbackRejectedReason::RestoreFailed)),
        EmuResponse::TasControlRollbackRejected {
            requested_lease_id: 1,
            reason: TasControlRollbackRejectedReason::RestoreFailed,
        }
    ));
    assert!(control.is_leased());

    assert!(matches!(
        control.rollback(1, |checkpoint| Ok(TasRestoredCheckpoint {
            state_sha256: crate::tas_project::TasDigest([9; 32]),
            frame_count: checkpoint.frame_count,
        })),
        EmuResponse::TasControlRollbackRejected {
            reason: TasControlRollbackRejectedReason::StateDigestMismatch,
            ..
        }
    ));
    assert!(control.is_leased());

    assert!(matches!(
        control.rollback(1, |checkpoint| Ok(TasRestoredCheckpoint {
            state_sha256: checkpoint.state_sha256,
            frame_count: checkpoint.frame_count + 1,
        })),
        EmuResponse::TasControlRollbackRejected {
            reason: TasControlRollbackRejectedReason::FrameCountMismatch,
            ..
        }
    ));
    assert!(control.is_leased());

    assert!(matches!(
        control.rollback(1, |checkpoint| Ok(TasRestoredCheckpoint {
            state_sha256: checkpoint.state_sha256,
            frame_count: checkpoint.frame_count,
        })),
        EmuResponse::TasControlRolledBack { lease_id: 1, .. }
    ));
    assert!(!control.is_leased());
}

fn assert_replay_command_latches(command: EmuCommand) {
    let mut control = TasControl::new();
    assert!(matches!(
        control.dispatch(command, unblocked_context(), |_| unreachable!()),
        ControlFlow::Continue(_)
    ));
    assert!(matches!(
        control.dispatch(
            EmuCommand::AcquireTasControl {
                request_id: 91,
                profile: TasExecutionProfile::DirectNesCartridge,
            },
            unblocked_context(),
            |_| Ok(dummy_witness()),
        ),
        ControlFlow::Break(EmuResponse::TasControlAcquireRejected {
            request_id: 91,
            reason: TasControlAcquireRejectedReason::ReplayActivityUnwitnessed,
        })
    ));
}

#[test]
fn every_worker_observable_replay_path_permanently_latches_refusal() {
    assert_replay_command_latches(EmuCommand::CaptureReplayStart { capture_id: 1 });
    assert_replay_command_latches(EmuCommand::CaptureReplayCheckpoint { frame: 2 });

    let mut replay_frames = frame_input(1);
    replay_frames.replay_joypad_frames =
        Some(vec![crate::emu_thread::ReplayJoypadFrame::default()]);
    assert_replay_command_latches(EmuCommand::StepFrames(Box::new(replay_frames)));

    assert_replay_command_latches(EmuCommand::LoadStateBytes {
        state_bytes: vec![1],
        buttons_pressed: 0,
        dpad_pressed: 0,
        replay_events: Some(Vec::new()),
        game_boy_link_start_state: None,
        game_boy_link_coordinator_start_state: None,
        game_boy_link_start_tick: None,
        wonder_swan_link_start_tick: None,
    });
    assert_replay_command_latches(EmuCommand::RestoreGameBoyLinkState(
        zeff_emu_common::replay::ReplayGameBoyLinkState {
            peer_present: false,
            pending_master_byte: None,
            pending_master_response: None,
            pending_master_completion_ready: false,
            queued_master_action: None,
            pending_passive_completion: None,
            serial_generation: 0,
        },
    ));
}

#[test]
fn ordinary_worker_commands_do_not_latch_replay_refusal() {
    let mut control = TasControl::new();
    assert!(matches!(
        control.dispatch(
            EmuCommand::SetSampleRate(48_000),
            unblocked_context(),
            |_| unreachable!(),
        ),
        ControlFlow::Continue(_)
    ));
    assert!(matches!(
        control.dispatch(
            EmuCommand::AcquireTasControl {
                request_id: 92,
                profile: TasExecutionProfile::DirectNesCartridge,
            },
            unblocked_context(),
            |_| Ok(dummy_witness()),
        ),
        ControlFlow::Break(EmuResponse::TasControlAcquired {
            request_id: 92,
            lease_id: 1,
            ..
        })
    ));
}
