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
fn direct_ppf_multitap_suspend_authenticates_each_card_and_no_op_stack() {
    let syncs = crate::emu_backend::loader::direct_pce_multitap_cd_ppf_tas_sync_configs_for_test();
    for (index, (arcade, memory_base)) in [(false, false), (true, false), (false, true)]
        .into_iter()
        .enumerate()
    {
        for no_op in [false, true] {
            let name = format!("tas-repair-direct-ppf-card-{index}-no-op-{no_op}");
            let directory = crate::test_support::test_directory(&name).unwrap();
            let cue_path = directory.path().join("disc.cue");
            let mut disc = vec![0x71; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
            disc[..name.len()].copy_from_slice(name.as_bytes());
            std::fs::write(directory.path().join("disc.bin"), disc).unwrap();
            std::fs::write(
                &cue_path,
                b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
            )
            .unwrap();
            let source_hash =
                crate::emu_backend::pce_cd::load_direct_cue_with_mods(&cue_path, false)
                    .unwrap()
                    .source_disc_sha256;
            let controller_catalog =
                register_test_controller_catalog_hash(source_hash, PceControllerMode::Multitap);
            let arcade_catalog =
                arcade.then(|| register_test_arcade_card_catalog_hash(source_hash));
            let memory_catalog =
                memory_base.then(|| register_test_memory_base_catalog_hash(source_hash));
            let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
                &cue_path,
                vec![
                    (
                        "first.ppf".to_owned(),
                        ppf1(512, if no_op { 0x71 } else { 0x84 }),
                    ),
                    (
                        "second.ppf".to_owned(),
                        ppf1(513, if no_op { 0x71 } else { 0xA2 }),
                    ),
                ],
            )
            .unwrap();
            let system_card: &'static [u8] = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
            let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_and_ppf_stack(
                cue_path,
                system_card,
                zeff_firmware::sha256_bytes(system_card),
                stack,
            );
            let project = loader.create_project().unwrap();
            assert_eq!(project.identity().sync_config_sha256, syncs[index]);
            let identity = TasRepairIdentity {
                repair_id: 1,
                suspension_token: 1,
                project_content_sha256: TasDigest([0x11; 32]),
                profile: crate::emu_thread::TasExecutionProfile::DirectPceMultitapCd,
                source_media_sha256: project.identity().source_media_sha256,
                effective_media_sha256: project.identity().effective_media_sha256,
                required_sample_rate: 48_000,
                persistence: crate::emu_thread::TasPersistenceContract::Absent,
            };
            let backend = loader.load_fresh_backend().unwrap();
            let observation =
                crate::emu_thread::observe_tas_repair_profile(&backend, identity.profile);
            let provenance = backend.pce().unwrap().tas_load_provenance().unwrap();
            assert!(provenance.load.any_mod_enabled);
            assert_eq!(provenance.load.any_mod_applied, !no_op);
            assert_eq!(
                provenance.load.source_disc_sha256 == provenance.load.effective_disc_sha256,
                no_op
            );
            assert_eq!(observation.mods_absent, Some(false));
            validate_suspend_profile(identity, &observation, &backend).unwrap();

            let wrong_sync = mutate_provenance(loader.load_fresh_backend().unwrap(), |load| {
                load.tas_sync_config_sha256 = syncs[(index + 1) % syncs.len()].0;
            });
            assert_rejected(identity, &wrong_sync);
            let forged_route = mutate_provenance(loader.load_fresh_backend().unwrap(), |load| {
                load.direct_pce_cd_ppf = false;
            });
            assert_rejected(identity, &forged_route);
            let forged_source = mutate_provenance(loader.load_fresh_backend().unwrap(), |load| {
                load.source_disc_sha256 = Some([0xEF; 32]);
            });
            assert_rejected(identity, &forged_source);

            drop(controller_catalog);
            assert_rejected(identity, &backend);
            let _controller_catalog =
                register_test_controller_catalog_hash(source_hash, PceControllerMode::Multitap);
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
        panic!("PPF Multitap fixture must load PCE")
    };
    let mut provenance = pce.tas_load_provenance().unwrap().load.clone();
    mutate(&mut provenance);
    EmuBackend::Pce(Box::new((*pce).with_tas_load_provenance(provenance)))
}

fn ppf1(offset: u32, byte: u8) -> Vec<u8> {
    let mut patch = b"PPF10\0".to_vec();
    patch.resize(56, 0);
    patch.extend_from_slice(&offset.to_le_bytes());
    patch.extend_from_slice(&[1, byte]);
    patch
}
