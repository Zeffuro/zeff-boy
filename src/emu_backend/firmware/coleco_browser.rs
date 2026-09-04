use super::{
    ActiveSystem, ResolvedFirmwareBytes, firmware_candidate_summary,
    firmware_plan_for_active_system, resolve_from_inventory,
};

const COLECO_BIOS_ID: &str = "coleco.vision.bios";

pub(crate) fn resolve_coleco_bios_with_manifest(
    inventory: Option<&zeff_firmware::FirmwareInventory>,
    _firmware_roots: &[std::path::PathBuf],
    _content_path: Option<&std::path::Path>,
) -> anyhow::Result<ResolvedFirmwareBytes> {
    let plan = firmware_plan_for_active_system(ActiveSystem::Coleco);
    if let Some(resolved) = resolve_from_inventory(COLECO_BIOS_ID, &plan, inventory) {
        return Ok(resolved);
    }

    let candidate_summary = inventory.map_or_else(String::new, |inventory| {
        let resolution =
            zeff_firmware::FirmwareResolver::new(zeff_firmware::catalog_specs(), inventory)
                .resolve(&plan);
        resolution
            .entries
            .iter()
            .find(|entry| entry.request.id.as_ref() == COLECO_BIOS_ID)
            .map_or_else(String::new, |entry| {
                firmware_candidate_summary(&entry.candidates)
            })
    });

    anyhow::bail!(
        "ColecoVision requires the recognized 8192-byte retail BIOS ({COLECO_BIOS_ID}). Import coleco.rom, colecovision.rom, or BIOS.col in Settings > Firmware. {candidate_summary}"
    )
}
