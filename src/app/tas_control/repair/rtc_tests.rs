use super::*;
use crate::emu_backend::loader::{
    DirectGbTasExecutionLoader, DirectGbcTasExecutionLoader, PrivateTasExecutionLoader,
};
use crate::emu_thread::{
    EmuCommand, EmuResponse, RecoveryTestConfig, TasControlAcquireRejectedReason,
    TasExecutionCacheProof, TasExecutionRequest, TasRepairActionRejectedReason,
};
use crate::save_paths::recovery_state::{BatteryPublicationReceipt, decode_battery_generation};
use crate::test_support::write_zip;

#[derive(Clone, Copy)]
struct RtcSpec {
    cgb: bool,
    ram: bool,
    zip: bool,
}

struct RtcCase {
    _root: crate::test_support::TestDirectory,
    backend: EmuBackend,
    loader: PrivateTasExecutionLoader,
    project: crate::tas_project::TasProject,
    identity: TasRepairIdentity,
    save_path: std::path::PathBuf,
    generation_path: std::path::PathBuf,
    recovery_path: std::path::PathBuf,
    ram_len: usize,
}

fn rtc_case(label: &str, spec: RtcSpec) -> RtcCase {
    let root = crate::test_support::test_directory(label).unwrap();
    let extension = if spec.cgb { "gbc" } else { "gb" };
    let member = format!("folder/clock.{extension}");
    let direct_name = format!("clock.{extension}");
    let source_path = root
        .path()
        .join(if spec.zip { "clock.zip" } else { &direct_name });
    let rom_path = if spec.zip {
        source_path.join(&member)
    } else {
        source_path.clone()
    };
    let save_path = source_path.with_extension("sav");
    let mut rom = crate::test_support::build_gb_test_rom();
    rom.resize(256 * 1024, 0);
    rom[0x143] = if spec.cgb { 0xC0 } else { 0 };
    rom[0x147] = if spec.ram { 0x10 } else { 0x0F };
    rom[0x148] = 0x03;
    rom[0x149] = if spec.ram { 0x03 } else { 0 };
    if spec.zip {
        write_zip(&source_path, &[(member.as_str(), rom.as_slice())]).unwrap();
    } else {
        std::fs::write(&source_path, &rom).unwrap();
    }
    let loader = if spec.cgb {
        let loader = if spec.zip {
            DirectGbcTasExecutionLoader::new_zip(
                source_path.clone(),
                Some(rom_path.clone()),
                Vec::new(),
            )
        } else {
            DirectGbcTasExecutionLoader::new(source_path.clone(), Vec::new())
        };
        PrivateTasExecutionLoader::DirectGbc(loader)
    } else {
        let loader = if spec.zip {
            DirectGbTasExecutionLoader::new_zip(
                source_path.clone(),
                Some(rom_path.clone()),
                Vec::new(),
            )
        } else {
            DirectGbTasExecutionLoader::new(source_path.clone(), Vec::new())
        };
        PrivateTasExecutionLoader::DirectGb(loader)
    };
    let project = match &loader {
        PrivateTasExecutionLoader::DirectGb(loader) => loader.create_project().unwrap(),
        PrivateTasExecutionLoader::DirectGbc(loader) => loader.create_project().unwrap(),
        _ => unreachable!(),
    };
    let profile = if spec.cgb {
        TasExecutionProfile::DirectGbCartridgeCgb
    } else {
        TasExecutionProfile::DirectGbCartridgeDmg
    };
    let mut backend = loader.load_repair_backend(&project).unwrap();
    let persistence = persistence_contract_for_project(&project, &backend, profile).unwrap();
    backend.step_frame();
    let identity = TasRepairIdentity {
        repair_id: 41,
        suspension_token: 73,
        project_content_sha256: TasDigest([0xA5; 32]),
        profile,
        source_media_sha256: project.identity().source_media_sha256,
        effective_media_sha256: project.identity().effective_media_sha256,
        required_sample_rate: 48_000,
        persistence,
    };
    RtcCase {
        generation_path: root.path().join("generation.bin"),
        recovery_path: root.path().join("recovery.zst"),
        _root: root,
        backend,
        loader,
        project,
        identity,
        save_path,
        ram_len: if spec.ram { 32 * 1024 } else { 0 },
    }
}

fn receipt_for_sidecar(bytes: &[u8], ram_len: usize) -> BatteryPublicationReceipt {
    if ram_len == 0 {
        BatteryPublicationReceipt::from_components(&[(crate::save_paths::GB_RTC_COMPONENT, bytes)])
    } else {
        BatteryPublicationReceipt::from_components(&[
            (crate::save_paths::SRAM_COMPONENT, &bytes[..ram_len]),
            (crate::save_paths::GB_RTC_COMPONENT, &bytes[ram_len..]),
        ])
    }
}

fn recovery(case: &RtcCase, fail_generation_write: bool) -> RecoveryTestConfig {
    RecoveryTestConfig {
        generation_path: case.generation_path.clone(),
        state_path: case.recovery_path.clone(),
        fail_generation_write,
    }
}

const DMG_TIMER_DIRECT: RtcSpec = RtcSpec {
    cgb: false,
    ram: false,
    zip: false,
};
const DMG_RAM_ZIP: RtcSpec = RtcSpec {
    cgb: false,
    ram: true,
    zip: true,
};
const CGB_TIMER_ZIP: RtcSpec = RtcSpec {
    cgb: true,
    ram: false,
    zip: true,
};
const CGB_RAM_DIRECT: RtcSpec = RtcSpec {
    cgb: true,
    ram: true,
    zip: false,
};

#[test]
fn gb_rtc_app_prepare_worker_acquire_execute_and_rollback_are_exact() {
    for (label, spec) in [
        ("gb-rtc-acquire-dmg-timer-direct", DMG_TIMER_DIRECT),
        ("gb-rtc-acquire-dmg-ram-zip", DMG_RAM_ZIP),
        ("gb-rtc-acquire-cgb-timer-zip", CGB_TIMER_ZIP),
        ("gb-rtc-acquire-cgb-ram-direct", CGB_RAM_DIRECT),
    ] {
        let case = rtc_case(label, spec);
        let profile = case.identity.profile;
        let persistence = case.identity.persistence;
        let sync_identity_sha256 = case.project.identity().sync_config_sha256;
        let prepared_backend = case.loader.load_repair_backend(&case.project).unwrap();
        let mismatch = match persistence {
            TasPersistenceContract::GbRtcBattery {
                persistent_state,
                rtc_state,
                byte_len,
                target_baseline,
                ..
            } => TasPersistenceContract::GbRtcBattery {
                persistent_state,
                rtc_state,
                byte_len,
                initial_sha256: TasDigest([0xE7; 32]),
                target_baseline,
            },
            _ => unreachable!(),
        };
        assert_eq!(
            crate::emu_thread::build_tas_repair_witness_for_persistence(
                &prepared_backend,
                profile,
                mismatch,
            ),
            Err(TasControlAcquireRejectedReason::StateWitnessUnavailable)
        );

        let mut manager = TasRepairManager::new();
        let prepared = manager
            .prepare(
                TasRepairTarget {
                    project_content_sha256: case.identity.project_content_sha256,
                    profile,
                    source_media_sha256: case.identity.source_media_sha256,
                    effective_media_sha256: case.identity.effective_media_sha256,
                    required_sample_rate: 48_000,
                    persistence,
                },
                prepared_backend,
            )
            .unwrap();
        let identity = prepared.identity;
        let worker = EmuThread::try_spawn_repaired(prepared.backend, identity).unwrap();
        assert!(worker.send_checked(EmuCommand::InspectTasReadiness {
            request_id: 90,
            profile,
        }));
        let observation = match worker.recv_checked().unwrap() {
            EmuResponse::TasReadinessObserved {
                request_id: 90,
                observation,
            } => observation,
            _ => panic!("unexpected RTC readiness response"),
        };
        assert_eq!(
            crate::app::tas_control::readiness::evaluate_for_test(
                2,
                case.project.identity(),
                &observation,
                48_000,
            )
            .status,
            crate::app::tas_control::readiness::TasReadinessStatus::Ready
        );
        assert!(worker.send_checked(EmuCommand::AcquireTasControl {
            request_id: 91,
            profile,
        }));
        let (lease_id, witness) = match worker.recv_checked().unwrap() {
            EmuResponse::TasControlAcquired {
                request_id: 91,
                lease_id,
                witness,
            } => (lease_id, witness),
            _ => panic!("unexpected RTC acquisition response"),
        };
        assert!(worker.send_checked(EmuCommand::ExecuteTasControl(Box::new(
            TasExecutionRequest {
                profile,
                lease_id,
                run_id: 1,
                cache_proof: TasExecutionCacheProof {
                    sync_identity_sha256,
                    branch_prefix_sha256: TasDigest::from_bytes(&[]),
                    target_cursor: 0,
                },
                intermediate_cache_proofs: Vec::new(),
                predecessor_window: None,
                start_state_bytes: witness.current_state_bytes.clone(),
                input_prefix: Vec::new(),
            }
        ))));
        assert!(matches!(
            worker.recv_checked().unwrap(),
            EmuResponse::TasExecutionCompleted {
                profile: completed_profile,
                lease_id: completed_lease,
                run_id: 1,
                ..
            } if completed_profile == profile && completed_lease == lease_id
        ));
        assert!(worker.send_checked(EmuCommand::RollbackTasControl { lease_id }));
        assert!(matches!(
            worker.recv_checked().unwrap(),
            EmuResponse::TasControlRolledBack {
                lease_id: rolled_back,
                ..
            } if rolled_back == lease_id
        ));
        worker.discard_repaired_for_tas_restore(identity).unwrap();
    }
}

#[test]
fn gb_rtc_keep_uses_exact_receipt_for_timer_only_and_ram_plus_rtc() {
    for (label, spec) in [
        ("gb-rtc-keep-dmg-timer-direct", DMG_TIMER_DIRECT),
        ("gb-rtc-keep-dmg-ram-zip", DMG_RAM_ZIP),
        ("gb-rtc-keep-cgb-timer-zip", CGB_TIMER_ZIP),
        ("gb-rtc-keep-cgb-ram-direct", CGB_RAM_DIRECT),
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
            panic!("RTC Keep should publish durably");
        };
        let bytes = std::fs::read(&case.save_path).unwrap();
        assert_eq!(bytes.len(), case.ram_len + 64);
        assert_eq!(&bytes[case.ram_len + 48..case.ram_len + 56], b"ZBRTC001");
        let receipt = receipt_for_sidecar(&bytes, case.ram_len);
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
            crate::emu_thread::inspect_freshness_for_test(&reloaded, fresh_recovery,).unwrap(),
            crate::save_paths::recovery_state::RecoveryFreshness::Fresh
        );
        let mut mismatch = bytes;
        mismatch[case.ram_len] ^= 0x01;
        std::fs::write(&case.save_path, mismatch).unwrap();
        assert_eq!(
            crate::emu_thread::inspect_freshness_for_test(&reloaded, mismatch_recovery).unwrap(),
            crate::save_paths::recovery_state::RecoveryFreshness::Unknown
        );
    }
}

#[test]
fn gb_rtc_conflict_preserves_sidecar_and_repaired_restore_authority() {
    for (label, spec) in [
        ("gb-rtc-conflict-dmg-timer", DMG_TIMER_DIRECT),
        ("gb-rtc-conflict-cgb-ram", CGB_RAM_DIRECT),
    ] {
        let case = rtc_case(label, spec);
        let conflict = vec![0xD7; case.ram_len + 64];
        std::fs::write(&case.save_path, &conflict).unwrap();
        let recovery = recovery(&case, false);
        let worker =
            EmuThread::try_spawn_repaired_with_recovery(case.backend, case.identity, recovery)
                .unwrap();
        let outcome = worker
            .commit_repaired_tas_worker(case.identity, false)
            .unwrap();
        assert!(matches!(
            outcome,
            TasPersistencePublicationOutcome::NotPublished { .. }
        ));
        assert_eq!(std::fs::read(&case.save_path).unwrap(), conflict);
        worker
            .discard_repaired_for_tas_restore(case.identity)
            .unwrap();
    }
}

#[test]
fn gb_rtc_uncertain_publication_keeps_repaired_ownership() {
    for (label, spec) in [
        ("gb-rtc-uncertain-dmg-ram-zip", DMG_RAM_ZIP),
        ("gb-rtc-uncertain-cgb-timer-zip", CGB_TIMER_ZIP),
    ] {
        let case = rtc_case(label, spec);
        let recovery = recovery(&case, true);
        let worker =
            EmuThread::try_spawn_repaired_with_recovery(case.backend, case.identity, recovery)
                .unwrap();
        let outcome = worker
            .commit_repaired_tas_worker(case.identity, false)
            .unwrap();
        assert!(matches!(
            outcome,
            TasPersistencePublicationOutcome::PublishedDurabilityUncertain { .. }
        ));
        let bytes = std::fs::read(&case.save_path).unwrap();
        assert_eq!(bytes.len(), case.ram_len + 64);
        assert_eq!(&bytes[case.ram_len + 48..case.ram_len + 56], b"ZBRTC001");
        assert!(!case.generation_path.exists());
        assert_eq!(
            worker.commit_repaired_tas_worker(case.identity, false),
            Err(TasRepairReleaseFailure::Rejected(
                TasRepairActionRejectedReason::NoMatchingRepair
            ))
        );
    }
}

#[test]
fn gb_rtc_restore_resumes_exact_timer_only_and_ram_plus_rtc_worker() {
    for (label, spec) in [
        ("gb-rtc-restore-dmg-timer", DMG_TIMER_DIRECT),
        ("gb-rtc-restore-dmg-ram-zip", DMG_RAM_ZIP),
        ("gb-rtc-restore-cgb-timer-zip", CGB_TIMER_ZIP),
        ("gb-rtc-restore-cgb-ram", CGB_RAM_DIRECT),
    ] {
        let case = rtc_case(label, spec);
        let expected_state = case.backend.encode_state_bytes().unwrap();
        let before = std::fs::read(&case.save_path).ok();
        let suspended =
            match EmuThread::spawn(case.backend, false).suspend_for_tas_repair(case.identity) {
                Ok(suspended) => suspended,
                Err(_) => panic!("RTC worker should suspend for repair"),
            };
        assert_eq!(
            suspended.proof().state_sha256,
            TasDigest::from_bytes(&expected_state)
        );
        let worker = suspended.resume().unwrap();
        assert!(worker.send_checked(EmuCommand::CaptureStateBytes));
        let EmuResponse::StateCaptured(state) = worker.recv_checked().unwrap() else {
            panic!("restored RTC worker should capture state");
        };
        assert_eq!(state, expected_state);
        assert_eq!(std::fs::read(&case.save_path).ok(), before);
    }
}
