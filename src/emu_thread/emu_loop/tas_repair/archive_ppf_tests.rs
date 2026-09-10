use std::io::Write;

use super::*;
use crate::emu_backend::EmuBackend;
use crate::emu_backend::loader::DirectPceCdTasExecutionLoader;
use crate::emu_backend::pce::PceTasLoadProvenance;
use crate::emu_backend::pce_profiles::{
    register_test_arcade_card_catalog_hash, register_test_controller_catalog_hash,
    register_test_memory_base_catalog_hash,
};
use zeff_pce_core::hardware::PceControllerMode;

#[test]
fn archive_ppf_suspend_authenticates_cards_controllers_and_no_op_stacks() {
    for multitap in [false, true] {
        for (arcade, memory_base) in [(false, false), (true, false), (false, true)] {
            for no_op in [false, true] {
                let name = format!("repair-archive-ppf-{multitap}-{arcade}-{memory_base}-{no_op}");
                let directory = crate::test_support::test_directory(&name).unwrap();
                let cue_path = directory.path().join("disc.cue");
                let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
                let mut disc = vec![0x63; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
                disc[..name.len()].copy_from_slice(name.as_bytes());
                std::fs::write(&cue_path, cue).unwrap();
                std::fs::write(directory.path().join("disc.bin"), &disc).unwrap();
                let source_hash =
                    crate::emu_backend::pce_cd::load_direct_cue_with_mods(&cue_path, false)
                        .unwrap()
                        .source_disc_sha256;
                let controller_catalog = multitap.then(|| {
                    register_test_controller_catalog_hash(source_hash, PceControllerMode::Multitap)
                });
                let arcade_catalog =
                    arcade.then(|| register_test_arcade_card_catalog_hash(source_hash));
                let memory_catalog =
                    memory_base.then(|| register_test_memory_base_catalog_hash(source_hash));
                let archive_path = directory.path().join("disc.zip");
                let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive_path).unwrap());
                let options = zip::write::SimpleFileOptions::default();
                writer.start_file("set/disc.cue", options).unwrap();
                writer.write_all(cue).unwrap();
                writer.start_file("set/disc.bin", options).unwrap();
                writer.write_all(&disc).unwrap();
                for (index, byte) in [0x84, 0xB2].into_iter().enumerate() {
                    writer
                        .start_file(format!("set/disc.ppf/{:04}.ppf", index + 1), options)
                        .unwrap();
                    writer
                        .write_all(&ppf1(
                            512 + index as u32,
                            &[if no_op { 0x63 } else { byte }],
                        ))
                        .unwrap();
                }
                writer.finish().unwrap();
                let system_card: &'static [u8] = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
                let loader = if multitap {
                    DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
                        archive_path,
                        system_card,
                        zeff_firmware::sha256_bytes(system_card),
                    )
                } else {
                    DirectPceCdTasExecutionLoader::new_with_system_card_override(
                        archive_path,
                        system_card,
                        zeff_firmware::sha256_bytes(system_card),
                    )
                };
                let project = loader.create_project().unwrap();
                let mut identity = repair_identity(&project);
                if multitap {
                    identity.profile = crate::emu_thread::TasExecutionProfile::DirectPceMultitapCd;
                }
                let backend = loader.load_fresh_backend().unwrap();
                let observation =
                    crate::emu_thread::observe_tas_repair_profile(&backend, identity.profile);
                let provenance = backend.pce().unwrap().tas_load_provenance().unwrap();
                assert_eq!(observation.mods_absent, Some(false));
                assert!(provenance.load.any_mod_enabled);
                assert_eq!(provenance.load.any_mod_applied, !no_op);
                assert_eq!(
                    provenance.load.source_disc_sha256 == provenance.load.effective_disc_sha256,
                    no_op
                );
                validate_suspend_profile(identity, &observation, &backend).unwrap();

                let wrong_controller_sync = if multitap {
                    crate::emu_backend::loader::direct_pce_cd_archive_ppf_tas_sync_configs_for_test(
                    )[0]
                } else {
                    crate::emu_backend::loader::direct_pce_multitap_cd_archive_ppf_tas_sync_configs_for_test()[0]
                };
                let forged = mutate_provenance(loader.load_fresh_backend().unwrap(), |load| {
                    load.tas_sync_config_sha256 = wrong_controller_sync.0;
                });
                assert_rejected(identity, &forged);
                let forged = mutate_provenance(loader.load_fresh_backend().unwrap(), |load| {
                    load.archive_ppf_patches.swap(0, 1);
                });
                assert_rejected(identity, &forged);
                let forged = mutate_provenance(loader.load_fresh_backend().unwrap(), |load| {
                    load.source_disc_sha256 = Some([0xEC; 32]);
                });
                assert_rejected(identity, &forged);
                let forged = mutate_provenance(loader.load_fresh_backend().unwrap(), |load| {
                    load.source_disc_sha256 = None;
                });
                assert_rejected(identity, &forged);
                let forged = mutate_provenance(loader.load_fresh_backend().unwrap(), |load| {
                    load.direct_pce_cd_archive_ppf = false;
                });
                assert_rejected(identity, &forged);

                drop(controller_catalog);
                if multitap {
                    assert_rejected(identity, &backend);
                }
                let _controller_catalog = multitap.then(|| {
                    register_test_controller_catalog_hash(source_hash, PceControllerMode::Multitap)
                });
                let observation =
                    crate::emu_thread::observe_tas_repair_profile(&backend, identity.profile);
                validate_suspend_profile(identity, &observation, &backend).unwrap();
                drop(arcade_catalog);
                drop(memory_catalog);
                if arcade || memory_base {
                    assert_rejected(identity, &backend);
                }
            }
        }
    }
}

#[test]
fn archive_ppf_no_op_suspend_requires_exact_provenance_and_sync() {
    let directory = crate::test_support::test_directory("tas-repair-archive-ppf-no-op").unwrap();
    let source_path = directory.path().join("disc.zip");
    write_no_op_archive_ppf(&source_path);
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let loader = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path,
        system_card,
        zeff_firmware::sha256_bytes(system_card),
    );
    let project = loader.create_project().unwrap();
    let identity = repair_identity(&project);

    let backend = loader.load_fresh_backend().unwrap();
    let observation = crate::emu_thread::observe_tas_repair_profile(
        &backend,
        crate::emu_thread::TasExecutionProfile::DirectPceCd,
    );
    assert_eq!(observation.mods_absent, Some(false));
    let provenance = backend
        .pce()
        .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
        .unwrap();
    assert!(provenance.load.any_mod_enabled);
    assert!(!provenance.load.any_mod_applied);
    assert_eq!(
        provenance.load.source_disc_sha256,
        provenance.load.effective_disc_sha256
    );
    validate_suspend_profile(identity, &observation, &backend).unwrap();

    let wrong_sync = mutate_provenance(loader.load_fresh_backend().unwrap(), |provenance| {
        provenance.tas_sync_config_sha256 = [0x5A; 32];
    });
    assert_rejected(identity, &wrong_sync);

    let forged = mutate_provenance(loader.load_fresh_backend().unwrap(), |provenance| {
        provenance.archive_ppf_patches.clear();
    });
    assert_rejected(identity, &forged);
}

fn repair_identity(project: &crate::tas_project::TasProject) -> TasRepairIdentity {
    TasRepairIdentity {
        repair_id: 1,
        suspension_token: 1,
        project_content_sha256: TasDigest([0x11; 32]),
        profile: crate::emu_thread::TasExecutionProfile::DirectPceCd,
        source_media_sha256: project.identity().source_media_sha256,
        effective_media_sha256: project.identity().effective_media_sha256,
        required_sample_rate: 48_000,
        persistence: crate::emu_thread::TasPersistenceContract::Absent,
    }
}

fn assert_rejected(identity: TasRepairIdentity, backend: &EmuBackend) {
    let observation = crate::emu_thread::observe_tas_repair_profile(backend, identity.profile);
    assert_eq!(
        validate_suspend_profile(identity, &observation, backend),
        Err(TasRepairSuspendRejectedReason::UnsafeLoadedProfile)
    );
}

fn mutate_provenance(
    backend: EmuBackend,
    mutate: impl FnOnce(&mut PceTasLoadProvenance),
) -> EmuBackend {
    let EmuBackend::Pce(pce) = backend else {
        panic!("archive PPF fixture must load PCE")
    };
    let mut provenance = pce.tas_load_provenance().unwrap().load.clone();
    mutate(&mut provenance);
    EmuBackend::Pce(Box::new((*pce).with_tas_load_provenance(provenance)))
}

fn write_no_op_archive_ppf(path: &std::path::Path) {
    let fill = 0xA5;
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("set/disc.cue", options).unwrap();
    writer
        .write_all(b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n")
        .unwrap();
    writer.start_file("set/disc.bin", options).unwrap();
    writer
        .write_all(&vec![
            fill;
            4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES
        ])
        .unwrap();
    writer.start_file("set/disc.ppf/0001.ppf", options).unwrap();
    writer.write_all(&ppf1(0, &[fill])).unwrap();
    writer.finish().unwrap();
}

fn ppf1(offset: u32, bytes: &[u8]) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&offset.to_le_bytes());
    patch.push(bytes.len() as u8);
    patch.extend_from_slice(bytes);
    patch
}
