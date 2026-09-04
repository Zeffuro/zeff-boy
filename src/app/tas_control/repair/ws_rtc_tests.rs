use super::*;
use crate::emu_backend::loader::{DirectWsTasExecutionLoader, PrivateTasExecutionLoader};
use crate::emu_thread::{
    EmuCommand, EmuResponse, RecoveryTestConfig, TasExecutionCacheProof, TasExecutionRequest,
    TasFrameAdvanceRequest, TasInputFrame, TasRepairActionRejectedReason,
};
use crate::save_paths::recovery_state::{
    BatteryPublicationReceipt, RecoveryFreshness, decode_battery_generation,
};
use crate::test_support::write_zip;
use zeff_ws_core::hardware::cartridge::{SaveKind, compute_footer_checksum};

#[derive(Clone, Copy)]
struct RtcSpec {
    zip: bool,
    save_byte: u8,
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

fn rtc_rom(save_byte: u8) -> Vec<u8> {
    let mut rom = vec![0x90; 128 * 1024];
    let reset = rom.len() - 16;
    rom[reset..reset + 5].copy_from_slice(&[0xEA, 0, 0, 0, 0xF0]);
    let footer = rom.len() - 10;
    rom[footer..].fill(0);
    rom[footer + 1] = 1;
    rom[footer + 4] = 1;
    rom[footer + 5] = save_byte;
    rom[footer + 6] = 1;
    rom[footer + 7] = 1;
    let checksum = compute_footer_checksum(&rom);
    rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    rom
}

fn rtc_case(label: &str, spec: RtcSpec) -> RtcCase {
    let root = crate::test_support::test_directory(label).unwrap();
    let source_path = root
        .path()
        .join(if spec.zip { "clock.zip" } else { "clock.wsc" });
    let save_path = source_path.with_extension("sav");
    let rom = rtc_rom(spec.save_byte);
    let loader = if spec.zip {
        let other = rtc_rom(0);
        write_zip(
            &source_path,
            &[("other.wsc", &other), ("games/clock.wsc", &rom)],
        )
        .unwrap();
        DirectWsTasExecutionLoader::new_zip(
            source_path.clone(),
            Some(source_path.join("games/clock.wsc")),
        )
    } else {
        std::fs::write(&source_path, rom).unwrap();
        DirectWsTasExecutionLoader::new(source_path.clone())
    };
    let backup_len = SaveKind::from_byte(spec.save_byte).size();
    if backup_len != 0 {
        std::fs::write(&save_path, vec![0x5A; backup_len]).unwrap();
    }
    let loader = PrivateTasExecutionLoader::DirectWs(loader);
    let project = match &loader {
        PrivateTasExecutionLoader::DirectWs(loader) => loader.create_project().unwrap(),
        _ => unreachable!(),
    };
    let mut backend = loader.load_repair_backend(&project).unwrap();
    let persistence = persistence_contract_for_project(
        &project,
        &backend,
        TasExecutionProfile::DirectWsCartridge,
    )
    .unwrap();
    let identity = TasRepairIdentity {
        repair_id: 61,
        suspension_token: 83,
        project_content_sha256: TasDigest([0xD1; 32]),
        profile: TasExecutionProfile::DirectWsCartridge,
        source_media_sha256: project.identity().source_media_sha256,
        effective_media_sha256: project.identity().effective_media_sha256,
        required_sample_rate: zeff_ws_core::emulator::DEFAULT_SAMPLE_RATE,
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
    crate::save_paths::aggregate_battery_receipt(
        bytes,
        backup_len,
        crate::save_paths::WS_BACKUP_COMPONENT,
        crate::save_paths::WS_RTC_COMPONENT,
    )
    .unwrap()
}

const TIMER_DIRECT: RtcSpec = RtcSpec {
    zip: false,
    save_byte: 0,
};
const TIMER_ZIP: RtcSpec = RtcSpec {
    zip: true,
    save_byte: 0,
};
const SRAM_DIRECT: RtcSpec = RtcSpec {
    zip: false,
    save_byte: 0x02,
};
const EEPROM_ZIP: RtcSpec = RtcSpec {
    zip: true,
    save_byte: 0x10,
};

#[test]
fn ws_rtc_worker_acquires_executes_advances_and_rolls_back() {
    for (label, spec) in [
        ("ws-rtc-worker-timer-direct", TIMER_DIRECT),
        ("ws-rtc-worker-timer-zip", TIMER_ZIP),
        ("ws-rtc-worker-sram-direct", SRAM_DIRECT),
        ("ws-rtc-worker-eeprom-zip", EEPROM_ZIP),
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
                zeff_ws_core::emulator::DEFAULT_SAMPLE_RATE,
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
        let request = |run_id| TasExecutionRequest {
            profile: case.identity.profile,
            lease_id,
            run_id,
            cache_proof: TasExecutionCacheProof {
                sync_identity_sha256: case.project.identity().sync_config_sha256,
                branch_prefix_sha256: TasDigest::from_bytes(&[]),
                target_cursor: 0,
            },
            intermediate_cache_proofs: Vec::new(),
            predecessor_window: None,
            start_state_bytes: start_state_bytes.clone(),
            input_prefix: Vec::new(),
        };
        assert!(worker.send_checked(EmuCommand::ExecuteTasControl(Box::new(request(1)))));
        let (frame_count, state_sha256) = match worker.recv_checked().unwrap() {
            EmuResponse::TasExecutionCompleted {
                frame_count,
                state_sha256,
                ..
            } => (frame_count, state_sha256),
            _ => panic!("unexpected execution response"),
        };
        assert!(worker.send_checked(EmuCommand::ExecuteTasControl(Box::new(request(2)))));
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
fn ws_rtc_keep_receipt_and_recovery_freshness_are_exact() {
    for (label, spec) in [
        ("ws-rtc-keep-timer-direct", TIMER_DIRECT),
        ("ws-rtc-keep-eeprom-zip", EEPROM_ZIP),
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
            panic!("WonderSwan RTC Keep should publish durably");
        };
        let mut bytes = std::fs::read(&case.save_path).unwrap();
        assert_eq!(bytes.len(), case.backup_len + 24);
        assert_eq!(&bytes[case.backup_len..case.backup_len + 8], b"ZBWSRTC1");
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
        bytes[case.backup_len + 9] ^= 1;
        std::fs::write(&case.save_path, bytes).unwrap();
        assert_eq!(
            crate::emu_thread::inspect_freshness_for_test(&reloaded, mismatch_recovery).unwrap(),
            RecoveryFreshness::Unknown
        );
    }
}

#[test]
fn ws_rtc_conflict_and_uncertain_publication_preserve_exact_authority() {
    let conflict = rtc_case("ws-rtc-conflict-sram", SRAM_DIRECT);
    let competing = vec![0xD3; conflict.backup_len + 24];
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

    let uncertain = rtc_case("ws-rtc-uncertain-timer-zip", TIMER_ZIP);
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
    assert_eq!(std::fs::read(&uncertain.save_path).unwrap().len(), 24);
    assert!(!uncertain.generation_path.exists());
    assert_eq!(
        worker.commit_repaired_tas_worker(uncertain.identity, false),
        Err(TasRepairReleaseFailure::Rejected(
            TasRepairActionRejectedReason::NoMatchingRepair
        ))
    );
}

#[test]
fn ws_rtc_restore_resumes_timer_and_backup_workers_exactly() {
    for (label, spec) in [
        ("ws-rtc-restore-timer", TIMER_DIRECT),
        ("ws-rtc-restore-eeprom", EEPROM_ZIP),
    ] {
        let case = rtc_case(label, spec);
        let expected_state = case.backend.encode_state_bytes().unwrap();
        let before = std::fs::read(&case.save_path).ok();
        let suspended =
            match EmuThread::spawn(case.backend, false).suspend_for_tas_repair(case.identity) {
                Ok(suspended) => suspended,
                Err(error) => panic!(
                    "WonderSwan RTC worker should suspend for repair: {:?}",
                    error.reason
                ),
            };
        assert_eq!(
            suspended.proof().state_sha256,
            TasDigest::from_bytes(&expected_state)
        );
        let worker = suspended.resume().unwrap();
        assert!(worker.send_checked(EmuCommand::CaptureStateBytes));
        let EmuResponse::StateCaptured(state) = worker.recv_checked().unwrap() else {
            panic!("restored WonderSwan RTC worker should capture state");
        };
        assert_eq!(state, expected_state);
        assert_eq!(std::fs::read(&case.save_path).ok(), before);
    }
}

#[test]
fn ws_rtc_periodic_flush_preserves_the_complete_extension() {
    for (label, spec) in [
        ("ws-rtc-flush-timer", TIMER_DIRECT),
        ("ws-rtc-flush-eeprom", EEPROM_ZIP),
    ] {
        let mut case = rtc_case(label, spec);
        assert!(case.backend.flush_battery_sram().unwrap().is_some());
        let bytes = std::fs::read(&case.save_path).unwrap();
        assert_eq!(bytes.len(), case.backup_len + 24);
        assert_eq!(&bytes[case.backup_len..case.backup_len + 8], b"ZBWSRTC1");
        assert_eq!(
            case.backend.battery_generation_receipt().unwrap(),
            receipt(&bytes, case.backup_len)
        );
    }
}
