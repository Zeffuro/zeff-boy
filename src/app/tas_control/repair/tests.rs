use std::sync::Arc;

use super::*;
use crate::emu_backend::loader::{DirectGbTasExecutionLoader, PrivateTasExecutionLoader};
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::{EmuCommand, EmuResponse, TasRepairAction, TasRepairActionRejectedReason};

fn backend(label: &str, rom: &[u8]) -> (crate::test_support::TestDirectory, EmuBackend) {
    let root = crate::test_support::test_directory(label).unwrap();
    let path = root.path().join("repair.nes");
    std::fs::write(&path, rom).unwrap();
    let backend = load_backend_from_rom_source(
        ActiveSystem::Nes,
        &path,
        &path,
        None,
        BackendLoadConfig {
            apply_mods: false,
            nes_load_battery_sram: false,
            ..BackendLoadConfig::default()
        },
    )
    .unwrap()
    .backend;
    (root, backend)
}

fn prepare(
    manager: &mut TasRepairManager,
    label: &str,
    rom: &[u8],
) -> (crate::test_support::TestDirectory, TasPreparedRepair) {
    let (root, backend) = backend(label, rom);
    let media = TasDigest::from_bytes(rom);
    let prepared = manager
        .prepare(
            TasRepairTarget {
                project_content_sha256: TasDigest([0x51; 32]),
                profile: TasExecutionProfile::DirectNesCartridge,
                source_media_sha256: media,
                effective_media_sha256: media,
                required_sample_rate: 48_000,
                persistence: crate::emu_thread::TasPersistenceContract::Absent,
            },
            backend,
        )
        .unwrap();
    (root, prepared)
}

fn acquire_witness(
    worker: &EmuThread,
    request_id: u64,
) -> crate::emu_thread::TasControlLeaseWitness {
    assert!(worker.send_checked(EmuCommand::AcquireTasControl {
        request_id,
        profile: TasExecutionProfile::DirectNesCartridge,
    }));
    match worker.recv_checked().unwrap() {
        EmuResponse::TasControlAcquired {
            request_id: actual,
            witness,
            ..
        } if actual == request_id => *witness,
        _ => panic!("unexpected TAS acquisition response"),
    }
}

#[test]
fn timer_only_gb_rtc_uses_complete_clock_persistence_contract() {
    let root = crate::test_support::test_directory("tas-repair-gb-rtc-candidate").unwrap();
    let path = root.path().join("clock.gb");
    let mut rom = crate::test_support::build_gb_test_rom();
    rom[0x147] = 0x0F;
    rom[0x149] = 0;
    std::fs::write(&path, rom).unwrap();
    let loader = DirectGbTasExecutionLoader::new(path, Vec::new());
    let project = loader.create_project().unwrap();
    assert_eq!(
        project.identity().persistent_state,
        crate::tas_project::TasExternalIdentity::Absent
    );
    assert!(matches!(
        project.identity().rtc_state,
        crate::tas_project::TasExternalIdentity::ExternalSha256(_)
    ));
    let backend = PrivateTasExecutionLoader::DirectGb(loader)
        .load_repair_backend(&project)
        .unwrap();
    let witness = crate::emu_backend::loader::gb_rtc_persistence_witness(&backend).unwrap();
    assert_eq!(
        witness.persistent_state,
        project.identity().persistent_state
    );
    assert_eq!(witness.rtc_state, project.identity().rtc_state);
    assert_eq!(witness.complete_byte_len, 64);

    let contract = persistence_contract_for_project(
        &project,
        &backend,
        TasExecutionProfile::DirectGbCartridgeDmg,
    )
    .unwrap();
    assert!(matches!(
        contract,
        TasPersistenceContract::GbRtcBattery {
            persistent_state: TasExternalIdentity::Absent,
            rtc_state,
            byte_len: 64,
            initial_sha256,
            ..
        } if rtc_state == witness.rtc_state && initial_sha256 == witness.complete_sha256
    ));
    let lease_witness = crate::emu_thread::build_tas_repair_witness_for_persistence(
        &backend,
        TasExecutionProfile::DirectGbCartridgeDmg,
        contract,
    )
    .unwrap();
    assert_eq!(
        lease_witness.current_state_sha256,
        TasDigest::from_bytes(&lease_witness.current_state_bytes)
    );
}

#[test]
fn repaired_backend_is_validated_before_original_worker_changes() {
    let rom = crate::test_support::build_nes_test_rom();
    let (_root, repaired) = backend("tas-repair-prebuild-mismatch", &rom);
    let mut manager = TasRepairManager::new();
    let media = TasDigest::from_bytes(&rom);

    assert!(matches!(
        manager.prepare(
            TasRepairTarget {
                project_content_sha256: TasDigest([0x31; 32]),
                profile: TasExecutionProfile::DirectNesCartridge,
                source_media_sha256: media,
                effective_media_sha256: TasDigest([0xFF; 32]),
                required_sample_rate: 48_000,
                persistence: crate::emu_thread::TasPersistenceContract::Absent,
            },
            repaired,
        ),
        Err(TasRepairPrepareFailure::EffectiveMediaMismatch)
    ));
    assert_eq!(manager.state(), TasRepairState::Detached);
}

#[test]
fn restore_retires_repaired_worker_and_resumes_exact_original_under_fresh_generation() {
    let rom = crate::test_support::build_nes_test_rom();
    let mut manager = TasRepairManager::new();
    let (_repaired_root, prepared) = prepare(&mut manager, "tas-repair-restore-new", &rom);
    let (_original_root, original_backend) = backend("tas-repair-restore-old", &rom);
    let original = EmuThread::spawn(original_backend, false);
    assert!(original.send_checked(EmuCommand::SetSampleRate(44_100)));

    let repaired = match manager.begin(prepared, 7, 8, original) {
        Ok(worker) => worker,
        Err(_) => panic!("repair should activate"),
    };
    let TasRepairState::RepairedDetached { original_proof, .. } = manager.state() else {
        panic!("repair transaction should retain the parked original");
    };
    let result = match manager.restore(repaired, 8, 9) {
        Ok(result) => result,
        Err(_) => panic!("repair restore should succeed"),
    };

    assert_eq!(result.worker_generation, 9);
    assert_eq!(&result.original_proof, original_proof.as_ref());
    assert_eq!(
        result.original_proof.loaded_profile.current_sample_rate,
        Some(44_100)
    );
    assert_eq!(result.repaired_release_warning, None);
    let published = result.worker.shared_framebuffer().load_full().unwrap();
    assert_eq!(
        TasDigest::from_bytes(published.as_slice()),
        original_proof.framebuffer_sha256
    );
    assert_eq!(published.len(), original_proof.framebuffer_len);
    assert!(result.worker.send_checked(EmuCommand::CaptureStateBytes));
    let EmuResponse::StateCaptured(state) = result.worker.recv_checked().unwrap() else {
        panic!("restored original should answer an ordinary state capture");
    };
    assert_eq!(TasDigest::from_bytes(&state), original_proof.state_sha256);
}

#[test]
fn source_change_after_prebuild_rejects_suspension_and_keeps_original_active() {
    let repaired_rom = crate::test_support::build_nes_test_rom();
    let mut changed_rom = repaired_rom.clone();
    let last = changed_rom.len() - 1;
    changed_rom[last] ^= 0x01;
    let mut manager = TasRepairManager::new();
    let (_repaired_root, prepared) = prepare(&mut manager, "tas-repair-source-new", &repaired_rom);
    let (_original_root, original_backend) = backend("tas-repair-source-old", &changed_rom);
    let original = EmuThread::spawn(original_backend, false);

    let failure = match manager.begin(prepared, 30, 31, original) {
        Ok(_) => panic!("changed source must not activate repair"),
        Err(failure) => failure,
    };
    assert_eq!(
        failure.reason,
        TasRepairBeginFailureReason::OriginalSuspend(TasRepairSuspendFailure::Rejected(
            crate::emu_thread::TasRepairSuspendRejectedReason::SourceMediaMismatch
        ))
    );
    assert_eq!(manager.state(), TasRepairState::Detached);
    let worker = failure.original_worker.unwrap();
    assert!(worker.send_checked(EmuCommand::CaptureStateBytes));
    assert!(matches!(
        worker.recv_checked().unwrap(),
        EmuResponse::StateCaptured(_)
    ));
}

#[test]
fn keep_discards_original_once_and_leaves_repaired_worker_active() {
    let rom = crate::test_support::build_nes_test_rom();
    let mut manager = TasRepairManager::new();
    let (_repaired_root, prepared) = prepare(&mut manager, "tas-repair-keep-new", &rom);
    let (_original_root, original_backend) = backend("tas-repair-keep-old", &rom);
    let original = EmuThread::spawn(original_backend, false);
    let repaired = match manager.begin(prepared, 11, 12, original) {
        Ok(worker) => worker,
        Err(_) => panic!("repair should activate"),
    };
    assert!(manager.connect_pending());

    let repaired = match manager.restore(repaired, 12, 12) {
        Err((TasRepairResolveFailure::InvalidResumeGeneration, Some(worker))) => *worker,
        _ => panic!("restore must require a fresh generation"),
    };
    assert!(matches!(
        manager.state(),
        TasRepairState::RepairedDetached { .. }
    ));
    assert!(manager.request_resolution(TasRepairResolution::Keep));
    assert!(!manager.connect_pending());
    assert!(manager.request_resolution(TasRepairResolution::Restore));
    assert_eq!(
        manager.take_pending_resolution(),
        Some(TasRepairResolution::Restore)
    );
    assert_eq!(manager.take_pending_resolution(), None);

    manager.keep(12, &repaired, false).unwrap();
    assert_eq!(manager.state(), TasRepairState::Detached);
    assert_eq!(
        manager.keep(12, &repaired, false),
        Err(TasRepairResolveFailure::NoActiveTransaction)
    );
    let witness = acquire_witness(&repaired, 82);
    assert_eq!(witness.source_media_sha256, TasDigest::from_bytes(&rom));
    assert!(repaired.send_checked(EmuCommand::RollbackTasControl { lease_id: 1 }));
    assert!(matches!(
        repaired.recv_checked().unwrap(),
        EmuResponse::TasControlRolledBack { lease_id: 1, .. }
    ));
}

#[test]
fn failed_repaired_spawn_resumes_original_before_returning_failure() {
    let rom = crate::test_support::build_nes_test_rom();
    let mut manager = TasRepairManager::new();
    let (_repaired_root, prepared) = prepare(&mut manager, "tas-repair-spawn-new", &rom);
    let (_original_root, original_backend) = backend("tas-repair-spawn-old", &rom);
    let original = EmuThread::spawn(original_backend, false);

    let failure = match manager.begin_with_spawn(prepared, 20, 21, original, |_, _| {
        Err(std::io::Error::other("synthetic spawn failure"))
    }) {
        Ok(_) => panic!("synthetic repaired spawn should fail"),
        Err(failure) => failure,
    };

    assert_eq!(
        failure.reason,
        TasRepairBeginFailureReason::RepairedSpawnFailed
    );
    assert_eq!(manager.state(), TasRepairState::Detached);
    let worker = failure.original_worker.unwrap();
    let witness = acquire_witness(&worker, 83);
    assert_eq!(witness.frame_count, 0);
    assert!(worker.send_checked(EmuCommand::RollbackTasControl { lease_id: 1 }));
    assert!(matches!(
        worker.recv_checked().unwrap(),
        EmuResponse::TasControlRolledBack { lease_id: 1, .. }
    ));
}

#[test]
fn parked_worker_rejects_stale_tokens_and_ordinary_commands_until_exact_discard() {
    let rom = crate::test_support::build_nes_test_rom();
    let mut manager = TasRepairManager::new();
    let (_repaired_root, prepared) = prepare(&mut manager, "tas-repair-stale-new", &rom);
    let identity = prepared.identity;
    let (_original_root, original_backend) = backend("tas-repair-stale-old", &rom);
    let worker = EmuThread::spawn(original_backend, false);
    assert!(worker.send_checked(EmuCommand::SuspendTasRepair { identity }));
    let proof = match worker.recv_checked().unwrap() {
        EmuResponse::TasRepairSuspended { proof } => *proof,
        _ => panic!("unexpected suspension response"),
    };
    let stale = TasRepairIdentity {
        suspension_token: identity.suspension_token + 1,
        ..identity
    };

    assert!(worker.send_checked(EmuCommand::ResumeTasRepair {
        identity: stale,
        expected_proof: Box::new(proof),
    }));
    assert!(matches!(
        worker.recv_checked().unwrap(),
        EmuResponse::TasRepairActionRejected {
            action: TasRepairAction::ResumeOriginal,
            reason: TasRepairActionRejectedReason::StaleToken,
            ..
        }
    ));
    assert!(worker.send_checked(EmuCommand::CaptureStateBytes));
    assert!(matches!(
        worker.recv_checked().unwrap(),
        EmuResponse::TasRepairActionRejected { .. }
    ));
    assert!(worker.send_checked(EmuCommand::DiscardTasRepair { identity }));
    assert!(matches!(
        worker.recv_checked().unwrap(),
        EmuResponse::TasRepairOriginalDiscarded {
            identity: discarded
        } if discarded == identity
    ));
}

#[test]
fn resume_proof_mismatch_is_terminal_and_never_reconstructs_original() {
    let rom = crate::test_support::build_nes_test_rom();
    let mut manager = TasRepairManager::new();
    let (_repaired_root, prepared) = prepare(&mut manager, "tas-repair-proof-new", &rom);
    let identity = prepared.identity;
    let (_original_root, original_backend) = backend("tas-repair-proof-old", &rom);
    let worker = EmuThread::spawn(original_backend, false);
    let shared = worker.shared_framebuffer().clone();
    let parked = match worker.suspend_for_tas_repair(identity) {
        Ok(parked) => parked,
        Err(_) => panic!("original worker should suspend"),
    };
    shared.store(Some(Arc::new(vec![0xA5; parked.proof().framebuffer_len])));

    assert!(matches!(
        parked.resume(),
        Err(TasRepairReleaseFailure::Rejected(
            TasRepairActionRejectedReason::FramebufferMismatch
        ))
    ));
}
