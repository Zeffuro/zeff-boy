use super::*;
use crate::emu_backend::loader::{DirectMdTasExecutionLoader, MdTasProfile};
use crate::emu_thread::{TasExecutionCacheProof, TasExecutionPredecessorWindow};
use crate::tas_project::TasProject;
use zeff_md_core::emulator::rollback::{MdRollbackConfig, MdRollbackHashes};

type Loop = super::super::super::EmuLoop;

fn prepare(
    pad: MdTasProfile,
) -> (
    crate::test_support::TestDirectory,
    DirectMdTasExecutionLoader,
    TasProject,
) {
    let directory = crate::test_support::test_directory("md-live-worker").unwrap();
    let source = directory.path().join("control.md");
    std::fs::write(&source, crate::netplay::md_fixture::rom()).unwrap();
    let loader = DirectMdTasExecutionLoader::new(source, pad);
    let mut project = loader.create_project().unwrap();
    project
        .edit_transaction(|edit| {
            edit.insert_frames("main", 1, 7)?;
            let input = input(pad);
            let mut frame = crate::tas_project::TasInputFrame::default();
            frame.players[0] = crate::tas_project::TasControllerInput {
                buttons: input.p1_buttons,
                dpad: input.p1_dpad,
            };
            frame.players[1] = crate::tas_project::TasControllerInput {
                buttons: input.p2_buttons,
                dpad: input.p2_dpad,
            };
            edit.set_input_range("main", 0, 8, frame)
        })
        .unwrap();
    (directory, loader, project)
}

fn input(pad: MdTasProfile) -> TasInputFrame {
    let (p1_buttons, p2_buttons) = if pad == MdTasProfile::ThreeButton {
        (0x41, 0x0a)
    } else {
        (0x15, 0x2e)
    };
    TasInputFrame {
        p1_buttons,
        p1_dpad: 1,
        p2_buttons,
        p2_dpad: 8,
        ..Default::default()
    }
}

fn profile(pad: MdTasProfile) -> TasExecutionProfile {
    crate::emu_backend::loader::md_product::md_execution_profile(pad)
}

fn proof(project: &TasProject, cursor: u64) -> TasExecutionCacheProof {
    let identity = project.seek_cache_identity("main", cursor).unwrap();
    TasExecutionCacheProof {
        sync_identity_sha256: identity.sync_identity_sha256,
        branch_prefix_sha256: identity.branch_prefix_sha256,
        target_cursor: cursor,
    }
}

fn request(
    project: &TasProject,
    pad: MdTasProfile,
    lease_id: u64,
    run_id: u64,
    cursor: u64,
) -> TasExecutionRequest {
    TasExecutionRequest {
        profile: profile(pad),
        lease_id,
        run_id,
        cache_proof: proof(project, cursor),
        intermediate_cache_proofs: Vec::new(),
        predecessor_window: None,
        start_state_bytes: project.start_state().to_vec(),
        input_prefix: (0..cursor)
            .map(|frame| {
                let input = project.branch("main").unwrap().input_at(frame);
                TasInputFrame {
                    p1_buttons: input.players[0].buttons,
                    p1_dpad: input.players[0].dpad,
                    p2_buttons: input.players[1].buttons,
                    p2_dpad: input.players[1].dpad,
                    ..Default::default()
                }
            })
            .collect(),
    }
}

fn acquire_md(
    worker: &mut Loop,
    rx: &crossbeam_channel::Receiver<EmuResponse>,
    pad: MdTasProfile,
) -> u64 {
    assert!(worker.handle_command(EmuCommand::AcquireTasControl {
        request_id: 401,
        profile: profile(pad)
    }));
    match rx.recv().unwrap() {
        EmuResponse::TasControlAcquired { lease_id, .. } => lease_id,
        _ => panic!("acquire"),
    }
}

fn hashes(backend: &crate::emu_backend::EmuBackend) -> MdRollbackHashes {
    let crate::emu_backend::EmuBackend::Md(md) = backend else {
        panic!()
    };
    MdRollbackConfig::capture(md.core())
        .unwrap()
        .boundary_hashes(md.core())
        .unwrap()
}

fn step(backend: &mut crate::emu_backend::EmuBackend, input: TasInputFrame) -> Vec<f32> {
    backend.set_input(input.p1_buttons, input.p1_dpad);
    backend.set_input_p2(input.p2_buttons, input.p2_dpad);
    backend.step_frame();
    let mut pcm = Vec::new();
    backend.drain_audio_samples_into(&mut pcm);
    pcm
}

fn same(actual: &crate::emu_backend::EmuBackend, expected: &crate::emu_backend::EmuBackend) {
    assert_eq!(
        actual.encode_state_bytes().unwrap(),
        expected.encode_state_bytes().unwrap()
    );
    assert_eq!(actual.display_framebuffer(), expected.display_framebuffer());
    assert_eq!(hashes(actual), hashes(expected));
}

#[test]
fn native_md_typed_worker_seeks_real_cache_suffix_reseeks_advances_pcm_and_commits_both_pads() {
    for pad in [MdTasProfile::ThreeButton, MdTasProfile::SixButton] {
        let (_directory, loader, project) = prepare(pad);
        let mut expected = loader.load_fresh_backend().unwrap();
        let (mut worker, rx) = tas_nes_test_loop_from_backend(loader.load_fresh_backend().unwrap());
        let lease = acquire_md(&mut worker, &rx, pad);
        let untouched = worker.backend.encode_state_bytes().unwrap();
        assert!(
            worker.handle_command(EmuCommand::ExecuteTasControl(Box::new(request(
                &project,
                pad,
                lease + 1,
                1,
                3
            ))))
        );
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasExecutionRejected {
                reason: Rejected::WrongLease { .. },
                ..
            }
        ));
        let mut foreign_profile = request(&project, pad, lease, 1, 3);
        foreign_profile.profile = if pad == MdTasProfile::ThreeButton {
            TasExecutionProfile::DirectMdSixButton
        } else {
            TasExecutionProfile::DirectMdThreeButton
        };
        assert!(worker.handle_command(EmuCommand::ExecuteTasControl(Box::new(foreign_profile))));
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasExecutionRejected {
                reason: Rejected::WrongExecutionProfile { .. },
                ..
            }
        ));
        assert_eq!(worker.backend.encode_state_bytes().unwrap(), untouched);
        for (index, cursor) in [3, 5, 3, 5].into_iter().enumerate() {
            let run = index as u64 + 1;
            let mut command = request(&project, pad, lease, run, cursor);
            if run == 2 {
                command.predecessor_window = Some(TasExecutionPredecessorWindow {
                    source_proofs: vec![proof(&project, 5), proof(&project, 3)],
                    input_start_cursor: 3,
                    input_frames: vec![input(pad); 2],
                });
            }
            assert!(worker.handle_command(EmuCommand::ExecuteTasControl(Box::new(command))));
            assert!(
                matches!(rx.recv().unwrap(), EmuResponse::TasExecutionCompleted { executed_project_frames, .. } if executed_project_frames == cursor)
            );
            if expected.frame_count() > cursor {
                expected = loader.load_fresh_backend().unwrap();
            }
            while expected.frame_count() < cursor {
                step(&mut expected, input(pad));
            }
            same(&worker.backend, &expected);
        }
        let (entries, hits, bytes) = worker.md_tas_cache_observation_for_test();
        assert!((2..=16).contains(&entries) && hits >= 3 && bytes <= 64 * 1024 * 1024);
        use zeff_md_core::emulator::rollback::MdAllocationKind as Kind;
        let graph = worker.md_tas_retained_graph_for_test();
        assert_eq!(graph.count(Kind::SnapshotLayout), entries as u64);
        assert!(graph.count(Kind::CoreLayout) >= entries as u64 + 3);
        assert!((1..=2).contains(&graph.count(Kind::Rom)));
        assert_eq!(
            graph.count(Kind::AudioSamples),
            graph.count(Kind::CoreLayout)
        );
        assert!(graph.bytes(Kind::AudioSamples) > 0);
        assert!(bytes > graph.requested_payload_bytes() + graph.collector_capacity_bytes());
        for advance_id in 1..=2 {
            let expected_pcm = step(&mut expected, input(pad));
            let before = worker.backend.encode_state_bytes().unwrap();
            assert!(
                worker.handle_command(EmuCommand::AdvanceTasControl(Box::new(
                    TasFrameAdvanceRequest {
                        profile: profile(pad),
                        lease_id: lease,
                        run_id: 4,
                        advance_id,
                        segment_id: 1,
                        expected_segment_frame_count: advance_id - 1,
                        expected_executed_project_frames: advance_id + 4,
                        expected_frame_count: advance_id + 4,
                        expected_state_sha256: TasDigest::from_bytes(&before),
                        input: input(pad),
                        snapshot: None,
                    }
                )))
            );
            match rx.recv().unwrap() {
                EmuResponse::TasFrameAdvanced { audio_samples, .. } => {
                    assert!(!audio_samples.is_empty() && audio_samples.iter().any(|v| *v != 0.0));
                    assert_eq!(audio_samples, expected_pcm);
                }
                _ => panic!("advance"),
            }
            same(&worker.backend, &expected);
        }
        assert!(worker.handle_command(EmuCommand::CommitTasControl { lease_id: lease }));
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasControlCommitted { .. }
        ));
        same(&worker.backend, &expected);
        for _ in 0..2 {
            assert_eq!(
                step(&mut worker.backend, input(pad)),
                step(&mut expected, input(pad))
            );
            same(&worker.backend, &expected);
        }
    }
}

#[test]
fn native_md_failed_first_execute_corrupt_cache_and_failed_promotion_keep_original_and_worker_fence()
 {
    for pad in [MdTasProfile::ThreeButton, MdTasProfile::SixButton] {
        let (_directory, loader, project) = prepare(pad);
        let mut backend = loader.load_fresh_backend().unwrap();
        backend.step_frame();
        let mut control = loader.load_fresh_backend().unwrap();
        control.step_frame();
        let crate::emu_backend::EmuBackend::Md(md) = &backend else {
            panic!()
        };
        let address = md.core() as *const _ as usize;
        let queued = md.core().pending_audio_frames();
        assert!(queued > 0);
        let original = backend.encode_state_bytes().unwrap();
        let (mut worker, rx) = tas_nes_test_loop_from_backend(backend);
        let history_pixels = worker.backend.display_framebuffer().0.to_vec();
        worker.rewind_buffer.advance_frames(1);
        worker.rewind_buffer.push(&original, &history_pixels);
        let history_capacity = worker.rewind_buffer.capacity();
        let lease = acquire_md(&mut worker, &rx, pad);
        let mut invalid = request(&project, pad, lease, 1, 3);
        invalid.start_state_bytes[0] ^= 1;
        assert!(worker.handle_command(EmuCommand::ExecuteTasControl(Box::new(invalid))));
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasExecutionRejected { .. }
        ));
        assert_eq!(worker.backend.encode_state_bytes().unwrap(), original);
        let crate::emu_backend::EmuBackend::Md(md) = &worker.backend else {
            panic!()
        };
        assert_eq!(md.core().pending_audio_frames(), queued);
        assert!(worker.handle_command(EmuCommand::RollbackTasControl { lease_id: lease }));
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasControlRolledBack { .. }
        ));
        let crate::emu_backend::EmuBackend::Md(md) = &worker.backend else {
            panic!()
        };
        assert_eq!(md.core() as *const _ as usize, address);
        let lease = acquire_md(&mut worker, &rx, pad);
        assert!(
            worker.handle_command(EmuCommand::ExecuteTasControl(Box::new(request(
                &project, pad, lease, 1, 3
            ))))
        );
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasExecutionCompleted { .. }
        ));
        let before = worker.backend.encode_state_bytes().unwrap();
        let before_hashes = hashes(&worker.backend);
        worker.corrupt_md_tas_cache_for_test();
        assert!(
            worker.handle_command(EmuCommand::ExecuteTasControl(Box::new(request(
                &project, pad, lease, 2, 3
            ))))
        );
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasExecutionRejected { .. }
        ));
        assert_eq!(worker.backend.encode_state_bytes().unwrap(), before);
        assert_eq!(hashes(&worker.backend), before_hashes);
        assert!(worker.handle_command(EmuCommand::RollbackTasControl { lease_id: lease }));
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasControlRolledBack { .. }
        ));
        let lease = acquire_md(&mut worker, &rx, pad);
        assert!(
            worker.handle_command(EmuCommand::ExecuteTasControl(Box::new(request(
                &project, pad, lease, 1, 3
            ))))
        );
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasExecutionCompleted { .. }
        ));
        worker
            .runtime_fault
            .latch(Some("worker finalization refusal".into()));
        assert!(worker.handle_command(EmuCommand::CommitTasControl { lease_id: lease }));
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasControlCommitRejected { .. }
        ));
        assert!(worker.tas_control.is_leased());
        assert_eq!(worker.rewind_buffer.capacity(), history_capacity);
        let history = worker.rewind_buffer.peek().unwrap();
        assert_eq!(history.state_bytes, original);
        assert_eq!(history.framebuffer, history_pixels);
        assert!(worker.handle_command(EmuCommand::Reset));
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasControlCommandRejected { .. }
        ));
        worker.runtime_fault = Default::default();
        assert!(worker.handle_command(EmuCommand::RollbackTasControl { lease_id: lease }));
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasControlRolledBack { .. }
        ));
        let mut actual_pcm = Vec::new();
        let mut control_pcm = Vec::new();
        worker.backend.drain_audio_samples_into(&mut actual_pcm);
        control.drain_audio_samples_into(&mut control_pcm);
        assert_eq!(actual_pcm, control_pcm);
        for _ in 0..2 {
            assert_eq!(
                step(&mut worker.backend, input(pad)),
                step(&mut control, input(pad))
            );
            same(&worker.backend, &control);
        }
    }
}

#[test]
fn native_md_failed_nonterminal_retirement_stays_owned_and_shutdown_closes_without_publication() {
    for pad in [MdTasProfile::ThreeButton, MdTasProfile::SixButton] {
        let (_directory, loader, _project) = prepare(pad);
        let mut backend = loader.load_fresh_backend().unwrap();
        backend.step_frame();
        let original_raw = backend.encode_state_bytes().unwrap();
        let crate::emu_backend::EmuBackend::Md(md) = &backend else {
            panic!()
        };
        let original = md.core() as *const _ as usize;
        let queue = md.core().pending_audio_frames();
        let (mut worker, rx) = tas_nes_test_loop_from_backend(backend);
        worker.save_recovery_on_shutdown = true;
        let _lease = acquire_md(&mut worker, &rx, pad);
        worker
            .runtime_fault
            .latch(Some("retirement verification failure".into()));
        assert!(worker.prepare_tas_control_retirement().is_err());
        assert!(worker.tas_control.is_leased());
        let crate::emu_backend::EmuBackend::Md(md) = &worker.backend else {
            panic!()
        };
        assert!(md.rollback_owned());
        assert_eq!(md.core() as *const _ as usize, original);
        assert_eq!(md.core().pending_audio_frames(), queue);
        assert_eq!(worker.backend.encode_state_bytes().unwrap(), original_raw);
        assert!(!worker.handle_command(EmuCommand::Shutdown));
        let mut failed = false;
        let mut recovery_failed = false;
        let mut closed = false;
        while let Ok(response) = rx.try_recv() {
            match response {
                EmuResponse::SramFlushFailed(_) => failed = true,
                EmuResponse::RecoverySaveFailed(_) => recovery_failed = true,
                EmuResponse::ShutdownComplete => closed = true,
                EmuResponse::SramFlushed(_) | EmuResponse::RecoverySaved(_) => {
                    panic!("fenced shutdown published state")
                }
                _ => {}
            }
        }
        assert!(failed && recovery_failed && closed);
        assert!(worker.tas_control.is_leased());
    }
}

#[test]
fn native_md_typed_cache_uses_existing_entry_eviction_and_actual_graph_budget() {
    for pad in [MdTasProfile::ThreeButton, MdTasProfile::SixButton] {
        let (_directory, loader, mut project) = prepare(pad);
        project
            .edit_transaction(|edit| edit.insert_frames("main", 8, 10))
            .unwrap();
        let (mut worker, rx) = tas_nes_test_loop_from_backend(loader.load_fresh_backend().unwrap());
        let lease = acquire_md(&mut worker, &rx, pad);
        for cursor in 0..18 {
            assert!(
                worker.handle_command(EmuCommand::ExecuteTasControl(Box::new(request(
                    &project,
                    pad,
                    lease,
                    cursor + 1,
                    cursor
                ))))
            );
            assert!(matches!(
                rx.recv().unwrap(),
                EmuResponse::TasExecutionCompleted { .. }
            ));
            let (entries, _, bytes) = worker.md_tas_cache_observation_for_test();
            assert!(entries <= 16 && bytes <= 64 * 1024 * 1024);
        }
        let (_, hits, _) = worker.md_tas_cache_observation_for_test();
        assert!(
            worker.handle_command(EmuCommand::ExecuteTasControl(Box::new(request(
                &project, pad, lease, 19, 0
            ))))
        );
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasExecutionCompleted {
                executed_project_frames: 0,
                ..
            }
        ));
        assert_eq!(worker.md_tas_cache_observation_for_test().1, hits);
        let peer_payload = worker.insert_md_tas_peer_capacity_case_for_test();
        assert!(peer_payload > 64 * 1024 * 1024);
        assert!(worker.md_tas_cache_observation_for_test().2 > 64 * 1024 * 1024);
        assert!(
            worker.handle_command(EmuCommand::ExecuteTasControl(Box::new(request(
                &project, pad, lease, 20, 0
            ))))
        );
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasExecutionCompleted { .. }
        ));
        assert!(worker.md_tas_cache_observation_for_test().2 <= 64 * 1024 * 1024);
        assert!(worker.handle_command(EmuCommand::RollbackTasControl { lease_id: lease }));
        assert!(matches!(
            rx.recv().unwrap(),
            EmuResponse::TasControlRolledBack { .. }
        ));
    }
}
