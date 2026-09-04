use super::*;

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_cue_arcade_multitap() -> Result<()> {
    let directory = test_directory("tas-cli-pce-cd-arcade-multitap")?;
    let source_path = directory.path().join("disc.cue");
    let mut disc = vec![0xDA; 4 * zeff_pce_core::hardware::CD_USER_SECTOR_BYTES];
    disc[0..4].copy_from_slice(&[0x41, 0x4D, 0xDA, 0x15]);
    std::fs::write(directory.path().join("disc.bin"), disc)?;
    std::fs::write(
        &source_path,
        b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
    )?;
    let system_card = Box::leak(vec![0; 256 * 1024].into_boxed_slice());
    let firmware_sha256 = zeff_firmware::sha256_bytes(system_card);
    let base = DirectPceCdTasExecutionLoader::new_with_system_card_override(
        source_path.clone(),
        system_card,
        firmware_sha256,
    );
    let disc_sha256 = base
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture disc");
    let _arcade_catalog =
        crate::emu_backend::pce_profiles::register_test_arcade_card_catalog_hash(disc_sha256);
    let _controller_catalog =
        crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
            disc_sha256,
            zeff_pce_core::hardware::PceControllerMode::Multitap,
        );
    let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_override(
        source_path,
        system_card,
        firmware_sha256,
    );
    assert_eq!(
        loader
            .load_fresh_backend()?
            .pce()
            .expect("PC Engine backend")
            .arcade_card_mode(),
        zeff_pce_core::hardware::PceArcadeCardMode::Enabled
    );
    verifies_and_exports_direct_pce_cd_multitap(directory.path(), loader)
}
