use std::io::Write;

use super::*;
use crate::emu_backend::EmuBackend;
use crate::emu_backend::loader::DirectPceCdTasExecutionLoader;
use crate::emu_backend::pce::PceTasLoadProvenance;

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
    let observation = crate::emu_thread::observe_tas_repair_profile(
        backend,
        crate::emu_thread::TasExecutionProfile::DirectPceCd,
    );
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
