use super::{ResolvedFirmwareBytes, resolve_from_inventory};

pub(crate) fn resolve_md_tmss_with_manifest(
    inventory: Option<&zeff_firmware::FirmwareInventory>,
    firmware_roots: &[std::path::PathBuf],
    content_path: Option<&std::path::Path>,
) -> anyhow::Result<ResolvedFirmwareBytes> {
    const ID: &str = "sega.md.tmss";
    let plan = zeff_firmware::firmware_plan_for_mega_drive_tmss(true);
    if let Some(resolved) = resolve_from_inventory(ID, &plan, inventory) {
        return Ok(resolved);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (resolved, dirs, candidates) =
            super::find_external_firmware(ID, &plan, firmware_roots, content_path)?;
        if let Some(resolved) = resolved {
            return Ok(resolved);
        }
        anyhow::bail!(
            "Mega Drive TMSS is enabled, but no recognized 2048-byte bios_MD.bin was found. Set Settings > Firmware > Firmware directory or import the boot ROM. Searched: {}. {}",
            super::searched_firmware_dirs(&dirs),
            candidates.join(" ")
        )
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (firmware_roots, content_path);
        anyhow::bail!(
            "Mega Drive TMSS is enabled, but no recognized 2048-byte bios_MD.bin was found. Import the boot ROM in Settings > Firmware."
        )
    }
}

pub(crate) fn resolve_md_svp_with_manifest(
    inventory: Option<&zeff_firmware::FirmwareInventory>,
    firmware_roots: &[std::path::PathBuf],
    content_path: Option<&std::path::Path>,
) -> anyhow::Result<ResolvedFirmwareBytes> {
    const ID: &str = "sega.md.svp.irom";
    let plan = [zeff_firmware::FirmwareRequest::new(
        ID,
        zeff_firmware::RequirementLevel::Required,
        zeff_firmware::FallbackKind::None,
        zeff_firmware::FirmwareDependency::RuntimeMapped,
    )
    .with_model("svp")];
    if let Some(resolved) = resolve_from_inventory(ID, &plan, inventory) {
        return Ok(resolved);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (resolved, dirs, candidates) =
            super::find_external_firmware(ID, &plan, firmware_roots, content_path)?;
        if let Some(resolved) = resolved {
            return Ok(resolved);
        }
        anyhow::bail!(
            "Functional SVP requires a recognized 2048-byte svp.bin. Set Settings > Firmware > Firmware directory or import the internal ROM. Searched: {}. {}",
            super::searched_firmware_dirs(&dirs),
            candidates.join(" ")
        )
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (firmware_roots, content_path);
        anyhow::bail!(
            "Functional SVP requires a recognized 2048-byte svp.bin. Import the internal ROM in Settings > Firmware."
        )
    }
}
