use super::*;

#[test]
fn direct_pce_cd_rejects_mutation_extensions_and_incompatible_runtime() -> Result<()> {
    let (directory, loader) = fixture("pce-cd-tas-reject")?;
    let project = loader.create_project()?;
    fs::write(directory.path().join("disc.bin"), vec![0xA5; 4 * 2048])?;
    assert!(loader.load_editor_engine(&project).is_err());

    for extension in ["zip", "chd", "iso"] {
        let path = directory.path().join(format!("disc.{extension}"));
        fs::write(&path, [])?;
        assert!(
            DirectPceCdTasExecutionLoader::new_with_system_card_override(
                path,
                Box::leak(vec![0; 256 * 1024].into_boxed_slice()),
                TEST_SYSTEM_CARD_SHA256,
            )
            .load_fresh_backend()
            .is_err()
        );
    }

    let (_topology_directory, loader) = fixture("pce-cd-tas-topology")?;
    let mut backend = loader.load_fresh_backend()?;
    let EmuBackend::Pce(pce) = &mut backend else {
        unreachable!();
    };
    pce.update_controller_mode(PceControllerMode::SixButton);
    assert!(validate_direct_pce_cd_tas_runtime(&backend, false).is_err());
    let mut backend = loader.load_fresh_backend()?;
    let EmuBackend::Pce(pce) = &mut backend else {
        unreachable!();
    };
    pce.update_memory_base_mode(PceMemoryBaseMode::Enabled);
    assert!(validate_direct_pce_cd_tas_runtime(&backend, false).is_err());
    assert!(validate_direct_pce_cd_tas_runtime(&loader.load_fresh_backend()?, true).is_err());
    assert_ne!(TasDigest(TEST_SYSTEM_CARD_SHA256), TasDigest([0; 32]));
    Ok(())
}

#[test]
fn combined_card_provenance_returns_an_error_without_panicking() -> Result<()> {
    let (_directory, loader) = fixture("pce-cd-tas-combined-cards")?;
    let backend = loader.load_fresh_backend()?;
    let EmuBackend::Pce(pce) = backend else {
        unreachable!();
    };
    let mut provenance = pce.tas_load_provenance().unwrap().load.clone();
    provenance.selected_arcade_card_mode = PceArcadeCardMode::Enabled;
    provenance.selected_memory_base_mode = PceMemoryBaseMode::Enabled;
    let backend = EmuBackend::Pce(Box::new(pce.with_tas_load_provenance(provenance)));
    let validation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        validate_direct_pce_cd_tas_runtime(&backend, false)
    }));
    assert!(validation.is_ok_and(|result| result.is_err()));
    Ok(())
}
