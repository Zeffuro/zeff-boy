use super::*;

#[test]
fn native_cli_two_pass_verifies_and_exports_direct_ppf_multitap() -> Result<()> {
    let directory = test_directory("tas-cli-pce-cd-ppf-multitap")?;
    let source_path = directory.path().join("disc.cue");
    std::fs::write(directory.path().join("disc.bin"), vec![0xD8; 4 * 2048])?;
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
    let source_disc_sha256 = base
        .load_fresh_backend()?
        .pce()
        .and_then(crate::emu_backend::PceBackend::normalized_disc_hash)
        .expect("fixture disc");
    let _catalog = crate::emu_backend::pce_profiles::register_test_controller_catalog_hash(
        source_disc_sha256,
        zeff_pce_core::hardware::PceControllerMode::Multitap,
    );
    let stack = crate::emu_backend::pce_cd::PceCdTasPpfStack::for_test(
        &source_path,
        vec![("headless.ppf".to_owned(), ppf1(0, &[0xA5]))],
    )?;
    let loader = DirectPceCdTasExecutionLoader::new_multitap_with_system_card_and_ppf_stack(
        source_path,
        system_card,
        firmware_sha256,
        stack,
    );
    verifies_and_exports_direct_pce_cd_multitap(directory.path(), loader)
}
