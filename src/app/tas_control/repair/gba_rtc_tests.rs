use super::*;
use crate::emu_backend::loader::{DirectGbaTasExecutionLoader, PrivateTasExecutionLoader};
use crate::emu_thread::{
    EmuCommand, EmuResponse, RecoveryTestConfig, TasExecutionCacheProof, TasExecutionRequest,
    TasFrameAdvanceRequest, TasInputFrame, TasRepairActionRejectedReason,
};
use crate::save_paths::recovery_state::{
    BatteryPublicationReceipt, RecoveryFreshness, decode_battery_generation,
};
use crate::test_support::write_zip;

#[derive(Clone, Copy)]
struct RtcSpec {
    zip: bool,
    flash: bool,
}

struct RtcCase {
    _root: crate::test_support::TestDirectory,
    loader: PrivateTasExecutionLoader,
    project: crate::tas_project::TasProject,
    identity: TasRepairIdentity,
    backend: EmuBackend,
    save_path: std::path::PathBuf,
    generation_path: std::path::PathBuf,
    recovery_path: std::path::PathBuf,
    backup_len: usize,
}

fn rtc_rom(flash: bool) -> Vec<u8> {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"BPEE");
    rom[0xB0..0xB2].copy_from_slice(b"01");
    rom[0xB2] = 0x96;
    if flash {
        rom.extend_from_slice(b"FLASH1M_V103");
    }
    rom
}

fn rtc_case(label: &str, spec: RtcSpec) -> RtcCase {
    let root = crate::test_support::test_directory(label).unwrap();
    let source_path = root
        .path()
        .join(if spec.zip { "clock.zip" } else { "clock.gba" });
    let save_path = source_path.with_extension("sav");
    let rom = rtc_rom(spec.flash);
    let loader = if spec.zip {
        write_zip(&source_path, &[("games/clock.gba", &rom)]).unwrap();
        DirectGbaTasExecutionLoader::new_zip(
            source_path.clone(),
            Some(source_path.join("games/clock.gba")),
        )
    } else {
        std::fs::write(&source_path, rom).unwrap();
        DirectGbaTasExecutionLoader::new(source_path.clone())
    };
    let backup_len = if spec.flash { 0x20000 } else { 0 };
    if backup_len != 0 {
        std::fs::write(&save_path, vec![0x5A; backup_len]).unwrap();
    }
    let loader = PrivateTasExecutionLoader::DirectGba(loader);
    let project = match &loader {
        PrivateTasExecutionLoader::DirectGba(loader) => loader.create_project().unwrap(),
        _ => unreachable!(),
    };
    let mut backend = loader.load_repair_backend(&project).unwrap();
    let persistence = persistence_contract_for_project(
        &project,
        &backend,
        TasExecutionProfile::DirectGbaCartridge,
    )
    .unwrap();
    let identity = TasRepairIdentity {
        repair_id: 53,
        suspension_token: 79,
        project_content_sha256: TasDigest([0xC1; 32]),
        profile: TasExecutionProfile::DirectGbaCartridge,
        source_media_sha256: project.identity().source_media_sha256,
        effective_media_sha256: project.identity().effective_media_sha256,
        required_sample_rate: crate::emu_backend::gba::DIRECT_GBA_SAMPLE_RATE,
        persistence,
    };
    backend.step_frame();
    RtcCase {
        generation_path: root.path().join("generation.bin"),
        recovery_path: root.path().join("recovery.zst"),
        _root: root,
        loader,
        project,
        identity,
        backend,
        save_path,
        backup_len,
    }
}

fn recovery(case: &RtcCase, fail_generation_write: bool) -> RecoveryTestConfig {
    RecoveryTestConfig {
        generation_path: case.generation_path.clone(),
        state_path: case.recovery_path.clone(),
        fail_generation_write,
    }
}

fn receipt(bytes: &[u8], backup_len: usize) -> BatteryPublicationReceipt {
    if backup_len == 0 {
        BatteryPublicationReceipt::from_components(&[(crate::save_paths::GBA_RTC_COMPONENT, bytes)])
    } else {
        BatteryPublicationReceipt::from_components(&[
            (
                crate::save_paths::GBA_BACKUP_COMPONENT,
                &bytes[..backup_len],
            ),
            (crate::save_paths::GBA_RTC_COMPONENT, &bytes[backup_len..]),
        ])
    }
}

const TIMER_DIRECT: RtcSpec = RtcSpec {
    zip: false,
    flash: false,
};
const TIMER_ZIP: RtcSpec = RtcSpec {
    zip: true,
    flash: false,
};
const FLASH_DIRECT: RtcSpec = RtcSpec {
    zip: false,
    flash: true,
};
const FLASH_ZIP: RtcSpec = RtcSpec {
    zip: true,
    flash: true,
};

#[test]
fn gba_rtc_worker_acquires_executes_advances_and_rolls_back() {
    for (label, spec) in [
        ("gba-rtc-worker-timer-direct", TIMER_DIRECT),
        ("gba-rtc-worker-timer-zip", TIMER_ZIP),
        ("gba-rtc-worker-flash-direct", FLASH_DIRECT),
        ("gba-rtc-worker-flash-zip", FLASH_ZIP),
    ] {
        let case = rtc_case(label, spec);
        let prepared = case.loader.load_repair_backend(&case.project).unwrap();
        let worker = EmuThread::try_spawn_repaired(prepared, case.identity).unwrap();
        assert!(worker.send_checked(EmuCommand::InspectTasReadiness {
            request_id: 1,
            profile: case.identity.profile,
        }));
        let observation = match worker.recv_checked().unwrap() {
            EmuResponse::TasReadinessObserved { observation, .. } => observation,
            _ => panic!("unexpected readiness response"),
        };
        assert_eq!(
            crate::app::tas_control::readiness::evaluate_for_test(
                2,
                case.project.identity(),
                &observation,
                crate::emu_backend::gba::DIRECT_GBA_SAMPLE_RATE,
            )
            .status,
            crate::app::tas_control::readiness::TasReadinessStatus::Ready
        );
        assert!(worker.send_checked(EmuCommand::AcquireTasControl {
            request_id: 2,
            profile: case.identity.profile,
        }));
        let (lease_id, witness) = match worker.recv_checked().unwrap() {
            EmuResponse::TasControlAcquired {
                lease_id, witness, ..
            } => (lease_id, witness),
            _ => panic!("unexpected acquire response"),
        };
        let start_state_bytes = witness.current_state_bytes;
        assert!(worker.send_checked(EmuCommand::ExecuteTasControl(Box::new(
            TasExecutionRequest {
                profile: case.identity.profile,
                lease_id,
                run_id: 1,
                cache_proof: TasExecutionCacheProof {
                    sync_identity_sha256: case.project.identity().sync_config_sha256,
                    branch_prefix_sha256: TasDigest::from_bytes(&[]),
                    target_cursor: 0,
                },
                intermediate_cache_proofs: Vec::new(),
                predecessor_window: None,
                start_state_bytes: start_state_bytes.clone(),
                input_prefix: Vec::new(),
            },
        ))));
        let (frame_count, state_sha256) = match worker.recv_checked().unwrap() {
            EmuResponse::TasExecutionCompleted {
                frame_count,
                state_sha256,
                ..
            } => (frame_count, state_sha256),
            _ => panic!("unexpected execution response"),
        };
        assert!(worker.send_checked(EmuCommand::ExecuteTasControl(Box::new(
            TasExecutionRequest {
                profile: case.identity.profile,
                lease_id,
                run_id: 2,
                cache_proof: TasExecutionCacheProof {
                    sync_identity_sha256: case.project.identity().sync_config_sha256,
                    branch_prefix_sha256: TasDigest::from_bytes(&[]),
                    target_cursor: 0,
                },
                intermediate_cache_proofs: Vec::new(),
                predecessor_window: None,
                start_state_bytes,
                input_prefix: Vec::new(),
            },
        ))));
        assert!(matches!(
            worker.recv_checked().unwrap(),
            EmuResponse::TasExecutionCompleted {
                run_id: 2,
                frame_count: cached_frame,
                state_sha256: cached_state,
                ..
            } if cached_frame == frame_count && cached_state == state_sha256
        ));
        assert!(worker.send_checked(EmuCommand::AdvanceTasControl(Box::new(
            TasFrameAdvanceRequest {
                profile: case.identity.profile,
                lease_id,
                run_id: 2,
                advance_id: 1,
                segment_id: 1,
                expected_segment_frame_count: 0,
                expected_executed_project_frames: 0,
                expected_frame_count: frame_count,
                expected_state_sha256: state_sha256,
                input: TasInputFrame::default(),
                snapshot: None,
            },
        ))));
        assert!(matches!(
            worker.recv_checked().unwrap(),
            EmuResponse::TasFrameAdvanced { frame_count: 1, .. }
        ));
        assert!(worker.send_checked(EmuCommand::RollbackTasControl { lease_id }));
        assert!(matches!(
            worker.recv_checked().unwrap(),
            EmuResponse::TasControlRolledBack { frame_count: 0, .. }
        ));
        worker
            .discard_repaired_for_tas_restore(case.identity)
            .unwrap();
    }
}

#[test]
fn gba_rtc_keep_receipt_and_recovery_freshness_are_exact() {
    for (label, spec) in [
        ("gba-rtc-keep-timer-direct", TIMER_DIRECT),
        ("gba-rtc-keep-flash-zip", FLASH_ZIP),
    ] {
        let case = rtc_case(label, spec);
        let recovery_config = recovery(&case, false);
        let fresh_recovery = recovery(&case, false);
        let mismatch_recovery = recovery(&case, false);
        let worker = EmuThread::try_spawn_repaired_with_recovery(
            case.backend,
            case.identity,
            recovery_config,
        )
        .unwrap();
        let outcome = worker
            .commit_repaired_tas_worker(case.identity, true)
            .unwrap();
        let TasPersistencePublicationOutcome::PublishedDurable {
            generation,
            component_sha256,
            ..
        } = outcome
        else {
            panic!("GBA RTC Keep should publish durably");
        };
        let bytes = std::fs::read(&case.save_path).unwrap();
        assert_eq!(bytes.len(), case.backup_len + 40);
        assert_eq!(&bytes[case.backup_len..case.backup_len + 8], b"ZBGARTC1");
        let receipt = receipt(&bytes, case.backup_len);
        assert_eq!(component_sha256, TasDigest(receipt.component_sha256));
        let record = decode_battery_generation(
            &std::fs::read(&case.generation_path).unwrap(),
            case.identity.effective_media_sha256.0,
        )
        .unwrap();
        assert_eq!(record.generation, generation);
        assert_eq!(record.component_sha256, receipt.component_sha256);
        let reloaded = case.loader.load_repair_backend(&case.project).unwrap();
        assert_eq!(
            crate::emu_thread::inspect_freshness_for_test(&reloaded, fresh_recovery).unwrap(),
            RecoveryFreshness::Fresh
        );
        let mut mismatched = bytes;
        mismatched[case.backup_len + 8] ^= 1;
        std::fs::write(&case.save_path, mismatched).unwrap();
        assert_eq!(
            crate::emu_thread::inspect_freshness_for_test(&reloaded, mismatch_recovery).unwrap(),
            RecoveryFreshness::Unknown
        );
    }
}

#[test]
fn gba_rtc_conflict_and_uncertain_publication_preserve_exact_authority() {
    let conflict = rtc_case("gba-rtc-conflict-flash-direct", FLASH_DIRECT);
    let competing = vec![0xD3; conflict.backup_len + 40];
    std::fs::write(&conflict.save_path, &competing).unwrap();
    let conflict_recovery = recovery(&conflict, false);
    let worker = EmuThread::try_spawn_repaired_with_recovery(
        conflict.backend,
        conflict.identity,
        conflict_recovery,
    )
    .unwrap();
    assert!(matches!(
        worker
            .commit_repaired_tas_worker(conflict.identity, false)
            .unwrap(),
        TasPersistencePublicationOutcome::NotPublished { .. }
    ));
    assert_eq!(std::fs::read(&conflict.save_path).unwrap(), competing);
    worker
        .discard_repaired_for_tas_restore(conflict.identity)
        .unwrap();

    let uncertain = rtc_case("gba-rtc-uncertain-timer-zip", TIMER_ZIP);
    let uncertain_recovery = recovery(&uncertain, true);
    let worker = EmuThread::try_spawn_repaired_with_recovery(
        uncertain.backend,
        uncertain.identity,
        uncertain_recovery,
    )
    .unwrap();
    assert!(matches!(
        worker
            .commit_repaired_tas_worker(uncertain.identity, false)
            .unwrap(),
        TasPersistencePublicationOutcome::PublishedDurabilityUncertain { .. }
    ));
    assert_eq!(std::fs::read(&uncertain.save_path).unwrap().len(), 40);
    assert!(!uncertain.generation_path.exists());
    assert_eq!(
        worker.commit_repaired_tas_worker(uncertain.identity, false),
        Err(TasRepairReleaseFailure::Rejected(
            TasRepairActionRejectedReason::NoMatchingRepair
        ))
    );
}

#[test]
fn gba_rtc_restore_resumes_timer_only_and_backup_workers_exactly() {
    for (label, spec) in [
        ("gba-rtc-restore-timer", TIMER_DIRECT),
        ("gba-rtc-restore-flash", FLASH_ZIP),
    ] {
        let case = rtc_case(label, spec);
        let expected_state = case.backend.encode_state_bytes().unwrap();
        let before = std::fs::read(&case.save_path).ok();
        let suspended =
            match EmuThread::spawn(case.backend, false).suspend_for_tas_repair(case.identity) {
                Ok(suspended) => suspended,
                Err(_) => panic!("GBA RTC worker should suspend for repair"),
            };
        assert_eq!(
            suspended.proof().state_sha256,
            TasDigest::from_bytes(&expected_state)
        );
        let worker = suspended.resume().unwrap();
        assert!(worker.send_checked(EmuCommand::CaptureStateBytes));
        let EmuResponse::StateCaptured(state) = worker.recv_checked().unwrap() else {
            panic!("restored GBA RTC worker should capture state");
        };
        assert_eq!(state, expected_state);
        assert_eq!(std::fs::read(&case.save_path).ok(), before);
    }
}
