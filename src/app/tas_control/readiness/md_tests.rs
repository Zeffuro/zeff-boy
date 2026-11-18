use super::*;
use crate::emu_backend::loader::{DirectMdTasExecutionLoader, MdTasProfile, md_product};

#[test]
fn native_md_readiness_binds_selected_member_even_when_rom_bytes_match() -> Result<()> {
    for pad in [MdTasProfile::ThreeButton, MdTasProfile::SixButton] {
        let directory = crate::test_support::test_directory("md-readiness-member")?;
        let source = directory.path().join("same.zip");
        let bytes = crate::netplay::md_fixture::rom();
        crate::test_support::write_zip(&source, &[("first.md", &bytes), ("second.gen", &bytes)])?;
        let selected =
            DirectMdTasExecutionLoader::new_zip(source.clone(), source.join("second.gen"), pad);
        let project = selected.create_project()?;
        let profile = md_product::md_execution_profile(pad);
        let wrong =
            DirectMdTasExecutionLoader::new_zip(source.clone(), source.join("first.md"), pad)
                .load_fresh_backend()?;
        let wrong_observation = md_product::observe_md_tas_profile(&wrong, false, profile);
        assert_eq!(
            wrong_observation.source_media_sha256,
            Some(project.identity().source_media_sha256)
        );
        assert_eq!(
            wrong_observation.effective_media_sha256,
            Some(project.identity().effective_media_sha256)
        );
        let wrong_report = evaluate(1, project.identity(), &wrong_observation, 48_000);
        assert_eq!(wrong_report.status, TasReadinessStatus::Incompatible);
        assert!(
            wrong_report
                .checks
                .iter()
                .any(|check| check.id.code == TasReadinessCode::Controllers
                    && check.status == TasReadinessStatus::Incompatible)
        );
        let correct = selected.load_fresh_backend()?;
        let observation = md_product::observe_md_tas_profile(&correct, false, profile);
        assert_eq!(
            observation.sync_config_sha256,
            Some(project.identity().sync_config_sha256)
        );
        assert_eq!(
            evaluate(1, project.identity(), &observation, 44_100).status,
            TasReadinessStatus::Ready
        );
    }
    Ok(())
}
