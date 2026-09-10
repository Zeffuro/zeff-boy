use super::*;

#[test]
fn only_exact_archive_ppf_or_multitap_ppf_identity_allows_loaded_mods() -> Result<()> {
    let (project, mut observation) = fixture()?;
    let mut identity = project.identity().clone();
    observation.mods_absent = Some(false);
    let witness = crate::tas_project::TasPatchIdentity {
        format: "pce-cd-unpatched-disc-v1".to_owned(),
        sha256: TasDigest([0xA5; 32]),
    };
    identity.patches = vec![witness.clone()];

    let multitap =
        crate::emu_backend::loader::direct_pce_multitap_cd_ppf_tas_sync_configs_for_test()
            .into_iter()
            .map(|sync| (TasExecutionProfile::DirectPceMultitapCd, sync));
    let archive = crate::emu_backend::loader::direct_pce_cd_archive_ppf_tas_sync_configs_for_test()
        .into_iter()
        .map(|sync| (TasExecutionProfile::DirectPceCd, sync));
    let archive_multitap =
        crate::emu_backend::loader::direct_pce_multitap_cd_archive_ppf_tas_sync_configs_for_test()
            .into_iter()
            .map(|sync| (TasExecutionProfile::DirectPceMultitapCd, sync));
    for (profile, sync) in multitap.chain(archive).chain(archive_multitap) {
        observation.profile = profile;
        identity.sync_config_sha256 = sync;
        identity.patches = vec![witness.clone()];
        assert_eq!(
            mods_status(&identity, &observation),
            TasReadinessStatus::Ready
        );

        identity.patches.clear();
        assert_incompatible(&identity, &observation);
        identity.patches = vec![witness.clone(), witness.clone()];
        assert_incompatible(&identity, &observation);
        identity.patches = vec![crate::tas_project::TasPatchIdentity {
            format: "wrong".to_owned(),
            sha256: witness.sha256,
        }];
        assert_incompatible(&identity, &observation);
        identity.patches = vec![witness.clone()];
        identity.sync_config_sha256 = TasDigest([0x5A; 32]);
        assert_incompatible(&identity, &observation);
        identity.sync_config_sha256 = sync;
        observation.profile = if profile == TasExecutionProfile::DirectPceMultitapCd {
            TasExecutionProfile::DirectPceCd
        } else {
            TasExecutionProfile::DirectPceMultitapCd
        };
        assert_incompatible(&identity, &observation);
        observation.profile = TasExecutionProfile::DirectPceHuCard;
        assert_incompatible(&identity, &observation);
    }
    Ok(())
}

fn mods_status(
    identity: &TasProjectIdentity,
    observation: &TasLoadedProfileObservation,
) -> TasReadinessStatus {
    evaluate(7, identity, observation, 48_000)
        .checks
        .into_iter()
        .find(|check| check.id.code == TasReadinessCode::Mods)
        .expect("mods check")
        .status
}

fn assert_incompatible(identity: &TasProjectIdentity, observation: &TasLoadedProfileObservation) {
    assert_eq!(
        mods_status(identity, observation),
        TasReadinessStatus::Incompatible
    );
}
