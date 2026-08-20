use super::ActiveSystem;

#[derive(Clone, Debug)]
pub(crate) struct ResolvedFirmwareBytes {
    pub(crate) bytes: Vec<u8>,
    pub(crate) manifest: zeff_emu_common::replay::ReplayFirmwareManifest,
}

pub(crate) fn firmware_plan_for_active_system(
    system: ActiveSystem,
) -> Vec<zeff_firmware::FirmwareRequest> {
    use zeff_firmware::ExistingCoreSystem;

    zeff_firmware::firmware_plan_for_existing_core(match system {
        ActiveSystem::GameBoy => ExistingCoreSystem::GameBoy,
        ActiveSystem::GameBoyAdvance => ExistingCoreSystem::GameBoyAdvance,
        ActiveSystem::Nes => ExistingCoreSystem::Nes,
        ActiveSystem::WonderSwan => ExistingCoreSystem::WonderSwan,
        ActiveSystem::MasterSystem => ExistingCoreSystem::MasterSystem,
        ActiveSystem::GameGear => ExistingCoreSystem::GameGear,
        ActiveSystem::Sg1000 => return Vec::new(),
    })
}

pub(crate) fn default_firmware_manifests_for_active_system(
    system: ActiveSystem,
) -> Vec<zeff_emu_common::replay::ReplayFirmwareManifest> {
    let plan = firmware_plan_for_active_system(system);
    zeff_firmware::FirmwareResolver::new(
        zeff_firmware::catalog_specs(),
        &zeff_firmware::FirmwareInventory::new(),
    )
    .resolve(&plan)
    .manifests()
    .into_iter()
    .map(replay_firmware_manifest)
    .collect()
}

fn resolve_from_inventory(
    firmware_id: &str,
    plan: &[zeff_firmware::FirmwareRequest],
    inventory: Option<&zeff_firmware::FirmwareInventory>,
) -> Option<ResolvedFirmwareBytes> {
    let inventory = inventory?;
    let resolution =
        zeff_firmware::FirmwareResolver::new(zeff_firmware::catalog_specs(), inventory)
            .resolve(plan);
    resolution.entries.into_iter().find_map(|entry| {
        (entry.request.id.as_ref() == firmware_id)
            .then_some(entry)
            .and_then(|entry| {
                let zeff_firmware::FirmwareSelection::External(firmware) = entry.selection else {
                    return None;
                };
                Some(ResolvedFirmwareBytes {
                    bytes: firmware.bytes.to_vec(),
                    manifest: replay_firmware_manifest(firmware.selection_manifest()),
                })
            })
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn resolve_fds_bios(
    inventory: Option<&zeff_firmware::FirmwareInventory>,
    firmware_roots: &[std::path::PathBuf],
    content_path: Option<&std::path::Path>,
) -> anyhow::Result<Vec<u8>> {
    Ok(resolve_fds_bios_with_manifest(inventory, firmware_roots, content_path)?.bytes)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn resolve_fds_bios_with_manifest(
    inventory: Option<&zeff_firmware::FirmwareInventory>,
    firmware_roots: &[std::path::PathBuf],
    content_path: Option<&std::path::Path>,
) -> anyhow::Result<ResolvedFirmwareBytes> {
    const FDS_BIOS_ID: &str = "nintendo.fds.bios";

    let plan = zeff_firmware::firmware_plan_for_famicom_disk_system();
    if let Some(resolved) = resolve_from_inventory(FDS_BIOS_ID, &plan, inventory) {
        return Ok(resolved);
    }
    let (resolved, search_dirs, candidate_summaries) =
        find_external_firmware(FDS_BIOS_ID, &plan, firmware_roots, content_path)?;
    if let Some(resolved) = resolved {
        return Ok(resolved);
    }

    let searched = if search_dirs.is_empty() {
        "no firmware directories were available".to_owned()
    } else {
        search_dirs
            .iter()
            .map(|dir| format!("{} (known firmware filenames only)", dir.path.display()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let candidate_summary = if candidate_summaries.is_empty() {
        String::new()
    } else {
        format!(" {}", candidate_summaries.join(" "))
    };

    anyhow::bail!(
        "Famicom Disk System firmware is required. No recognized {FDS_BIOS_ID} was found. Searched: {searched}. Expected a known 8192-byte disksys.rom. Set Settings > Firmware > Firmware directory; dedicated firmware roots named BIOS, firmware, or system also scan immediate system subfolders. Zeff-boy also checks non-recursive firmware folders near the loaded ROM.{}",
        candidate_summary
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn resolve_gba_bios_with_manifest(
    inventory: Option<&zeff_firmware::FirmwareInventory>,
    firmware_roots: &[std::path::PathBuf],
    content_path: Option<&std::path::Path>,
) -> anyhow::Result<ResolvedFirmwareBytes> {
    const GBA_BIOS_ID: &str = "nintendo.gba.bios";
    let plan = firmware_plan_for_active_system(ActiveSystem::GameBoyAdvance);
    if let Some(resolved) = resolve_from_inventory(GBA_BIOS_ID, &plan, inventory) {
        return Ok(resolved);
    }
    let (resolved, search_dirs, candidate_summaries) =
        find_external_firmware(GBA_BIOS_ID, &plan, firmware_roots, content_path)?;
    if let Some(resolved) = resolved {
        return Ok(resolved);
    }

    let searched = searched_firmware_dirs(&search_dirs);
    let candidates = candidate_summaries.join(" ");
    anyhow::bail!(
        "External GBA BIOS is enabled, but no recognized {GBA_BIOS_ID} was found. Searched: {searched}. Expected the known 16384-byte gba_bios.bin. {candidates}"
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn resolve_gb_boot_rom_with_manifest(
    firmware_id: &str,
    inventory: Option<&zeff_firmware::FirmwareInventory>,
    firmware_roots: &[std::path::PathBuf],
    content_path: Option<&std::path::Path>,
) -> anyhow::Result<ResolvedFirmwareBytes> {
    let plan = firmware_plan_for_active_system(ActiveSystem::GameBoy);
    if let Some(resolved) = resolve_from_inventory(firmware_id, &plan, inventory) {
        return Ok(resolved);
    }
    let (resolved, search_dirs, candidate_summaries) =
        find_external_firmware(firmware_id, &plan, firmware_roots, content_path)?;
    if let Some(resolved) = resolved {
        return Ok(resolved);
    }

    let searched = searched_firmware_dirs(&search_dirs);
    let candidates = candidate_summaries.join(" ");
    let expected = if firmware_id.ends_with(".cgb") {
        "the known 2304-byte cgb_boot.bin"
    } else {
        "the known 256-byte dmg_boot.bin"
    };
    anyhow::bail!(
        "External Game Boy boot ROM is enabled, but no recognized {firmware_id} was found. Searched: {searched}. Expected {expected}. {candidates}"
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn resolve_sega8_boot_rom_with_manifest(
    system: ActiveSystem,
    region: Option<zeff_sega8_core::hardware::region::Sega8Region>,
    inventory: Option<&zeff_firmware::FirmwareInventory>,
    firmware_roots: &[std::path::PathBuf],
    content_path: Option<&std::path::Path>,
) -> anyhow::Result<ResolvedFirmwareBytes> {
    let firmware_id = match system {
        ActiveSystem::MasterSystem => "sega.sms.boot",
        ActiveSystem::GameGear => "sega.gg.boot",
        _ => anyhow::bail!("{system:?} does not use Sega 8-bit boot firmware"),
    };
    let mut plan = firmware_plan_for_active_system(system);
    if system == ActiveSystem::MasterSystem
        && let Some(request) = plan
            .iter_mut()
            .find(|request| request.id.as_ref() == firmware_id)
    {
        request.region = region.map(|region| match region {
            zeff_sega8_core::hardware::region::Sega8Region::Export => "export".to_owned(),
            zeff_sega8_core::hardware::region::Sega8Region::Japanese
            | zeff_sega8_core::hardware::region::Sega8Region::JapanesePowerBaseConverter => {
                "japan".to_owned()
            }
        });
    }
    if let Some(resolved) = resolve_from_inventory(firmware_id, &plan, inventory) {
        return Ok(resolved);
    }
    let (resolved, search_dirs, candidate_summaries) =
        find_external_firmware(firmware_id, &plan, firmware_roots, content_path)?;
    if let Some(resolved) = resolved {
        return Ok(resolved);
    }

    anyhow::bail!(
        "External Sega boot ROM is enabled, but no recognized {firmware_id} was found. Searched: {}. {}",
        searched_firmware_dirs(&search_dirs),
        candidate_summaries.join(" ")
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn find_external_firmware(
    firmware_id: &str,
    plan: &[zeff_firmware::FirmwareRequest],
    firmware_roots: &[std::path::PathBuf],
    content_path: Option<&std::path::Path>,
) -> anyhow::Result<(
    Option<ResolvedFirmwareBytes>,
    Vec<FirmwareSearchDir>,
    Vec<String>,
)> {
    let catalog = zeff_firmware::catalog_specs();
    let expected_filenames = expected_filenames_for_plan(catalog, plan);
    let search_dirs = firmware_search_dirs(firmware_roots, content_path);
    let mut candidate_summaries = Vec::new();

    for search_dir in &search_dirs {
        let inventory =
            scan_known_firmware_filenames(&search_dir.path, &expected_filenames, catalog);
        if inventory.entries().is_empty() {
            continue;
        }

        let resolution = zeff_firmware::FirmwareResolver::new(catalog, &inventory).resolve(plan);
        for entry in &resolution.entries {
            if entry.request.id.as_ref() != firmware_id {
                continue;
            }
            if let zeff_firmware::FirmwareSelection::External(firmware) = &entry.selection {
                log::info!(
                    "Resolved {firmware_id} from {} in {}",
                    firmware.original_filename.as_deref().unwrap_or("<unknown>"),
                    search_dir.path.display()
                );
                return Ok((
                    Some(ResolvedFirmwareBytes {
                        bytes: firmware.bytes.to_vec(),
                        manifest: replay_firmware_manifest(firmware.selection_manifest()),
                    }),
                    search_dirs,
                    candidate_summaries,
                ));
            }

            let summary = firmware_candidate_summary(&entry.candidates);
            if !summary.is_empty() {
                candidate_summaries.push(format!("{}: {summary}", search_dir.path.display()));
            }
        }
    }

    Ok((None, search_dirs, candidate_summaries))
}

#[cfg(not(target_arch = "wasm32"))]
fn searched_firmware_dirs(search_dirs: &[FirmwareSearchDir]) -> String {
    if search_dirs.is_empty() {
        return "no firmware directories were available".to_owned();
    }
    search_dirs
        .iter()
        .map(|dir| format!("{} (known firmware filenames only)", dir.path.display()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn resolve_fds_bios(
    inventory: Option<&zeff_firmware::FirmwareInventory>,
    _firmware_roots: &[std::path::PathBuf],
    _content_path: Option<&std::path::Path>,
) -> anyhow::Result<Vec<u8>> {
    let plan = zeff_firmware::firmware_plan_for_famicom_disk_system();
    if let Some(resolved) = resolve_from_inventory("nintendo.fds.bios", &plan, inventory) {
        return Ok(resolved.bytes);
    }
    anyhow::bail!(
        "Famicom Disk System firmware is required, but browser firmware storage/import is not wired yet"
    )
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn resolve_fds_bios_with_manifest(
    inventory: Option<&zeff_firmware::FirmwareInventory>,
    _firmware_roots: &[std::path::PathBuf],
    _content_path: Option<&std::path::Path>,
) -> anyhow::Result<ResolvedFirmwareBytes> {
    let plan = zeff_firmware::firmware_plan_for_famicom_disk_system();
    if let Some(resolved) = resolve_from_inventory("nintendo.fds.bios", &plan, inventory) {
        return Ok(resolved);
    }
    anyhow::bail!(
        "Famicom Disk System firmware is required, but browser firmware storage/import is not wired yet"
    )
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn resolve_gba_bios_with_manifest(
    inventory: Option<&zeff_firmware::FirmwareInventory>,
    _firmware_roots: &[std::path::PathBuf],
    _content_path: Option<&std::path::Path>,
) -> anyhow::Result<ResolvedFirmwareBytes> {
    let plan = firmware_plan_for_active_system(ActiveSystem::GameBoyAdvance);
    if let Some(resolved) = resolve_from_inventory("nintendo.gba.bios", &plan, inventory) {
        return Ok(resolved);
    }
    anyhow::bail!("External GBA BIOS import is not available in the browser build")
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn resolve_gb_boot_rom_with_manifest(
    firmware_id: &str,
    inventory: Option<&zeff_firmware::FirmwareInventory>,
    _firmware_roots: &[std::path::PathBuf],
    _content_path: Option<&std::path::Path>,
) -> anyhow::Result<ResolvedFirmwareBytes> {
    let plan = firmware_plan_for_active_system(ActiveSystem::GameBoy);
    if let Some(resolved) = resolve_from_inventory(firmware_id, &plan, inventory) {
        return Ok(resolved);
    }
    anyhow::bail!("External Game Boy boot ROM import is not available in the browser build")
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn resolve_sega8_boot_rom_with_manifest(
    system: ActiveSystem,
    region: Option<zeff_sega8_core::hardware::region::Sega8Region>,
    inventory: Option<&zeff_firmware::FirmwareInventory>,
    _firmware_roots: &[std::path::PathBuf],
    _content_path: Option<&std::path::Path>,
) -> anyhow::Result<ResolvedFirmwareBytes> {
    let firmware_id = match system {
        ActiveSystem::MasterSystem => "sega.sms.boot",
        ActiveSystem::GameGear => "sega.gg.boot",
        _ => anyhow::bail!("{system:?} does not use Sega 8-bit boot firmware"),
    };
    let mut plan = firmware_plan_for_active_system(system);
    if system == ActiveSystem::MasterSystem
        && let Some(request) = plan
            .iter_mut()
            .find(|request| request.id.as_ref() == firmware_id)
    {
        request.region = region.map(|region| match region {
            zeff_sega8_core::hardware::region::Sega8Region::Export => "export".to_owned(),
            zeff_sega8_core::hardware::region::Sega8Region::Japanese
            | zeff_sega8_core::hardware::region::Sega8Region::JapanesePowerBaseConverter => {
                "japan".to_owned()
            }
        });
    }
    if let Some(resolved) = resolve_from_inventory(firmware_id, &plan, inventory) {
        return Ok(resolved);
    }
    anyhow::bail!("External Sega boot ROM import is not available in the browser build")
}

pub(crate) fn replay_firmware_manifest(
    manifest: zeff_firmware::FirmwareSelectionManifest,
) -> zeff_emu_common::replay::ReplayFirmwareManifest {
    match manifest {
        zeff_firmware::FirmwareSelectionManifest::External {
            firmware_id,
            variant,
            sha256,
        } => zeff_emu_common::replay::ReplayFirmwareManifest::External {
            firmware_id: firmware_id.as_ref().to_owned(),
            variant: variant.map(|variant| variant.as_ref().to_owned()),
            sha256,
        },
        zeff_firmware::FirmwareSelectionManifest::Hle {
            firmware_id,
            implementation,
            compatibility_version,
        } => zeff_emu_common::replay::ReplayFirmwareManifest::Hle {
            firmware_id: firmware_id.as_ref().to_owned(),
            implementation,
            compatibility_version,
        },
        zeff_firmware::FirmwareSelectionManifest::BuiltinOpenSource {
            firmware_id,
            implementation,
            compatibility_version,
            sha256,
        } => zeff_emu_common::replay::ReplayFirmwareManifest::BuiltinOpenSource {
            firmware_id: firmware_id.as_ref().to_owned(),
            implementation,
            compatibility_version,
            sha256,
        },
        zeff_firmware::FirmwareSelectionManifest::Skipped {
            firmware_id,
            compatibility_version,
        } => zeff_emu_common::replay::ReplayFirmwareManifest::Skipped {
            firmware_id: firmware_id.as_ref().to_owned(),
            compatibility_version,
        },
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq)]
struct FirmwareSearchDir {
    path: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
fn firmware_search_dirs(
    firmware_roots: &[std::path::PathBuf],
    content_path: Option<&std::path::Path>,
) -> Vec<FirmwareSearchDir> {
    let mut out = Vec::new();
    for root in firmware_roots {
        push_configured_firmware_search_dirs(&mut out, Some(root.clone()));
    }

    if let Some(content_parent) = content_path.and_then(std::path::Path::parent) {
        push_firmware_search_dir(&mut out, Some(content_parent.to_path_buf()));
        for name in FIRMWARE_ROOT_DIR_NAMES {
            push_firmware_search_dir(&mut out, Some(content_parent.join(name)));
        }

        if let Some(collection_parent) = content_parent.parent() {
            for name in FIRMWARE_ROOT_DIR_NAMES {
                push_firmware_search_dir(&mut out, Some(collection_parent.join(name)));
            }
        }
    }

    out
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn configured_firmware_inventory_dirs(
    firmware_roots: &[std::path::PathBuf],
) -> Vec<std::path::PathBuf> {
    firmware_search_dirs(firmware_roots, None)
        .into_iter()
        .map(|dir| dir.path)
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
const FIRMWARE_ROOT_DIR_NAMES: &[&str] = &["bios", "BIOS Files", "firmware", "system"];

#[cfg(not(target_arch = "wasm32"))]
fn push_configured_firmware_search_dirs(
    out: &mut Vec<FirmwareSearchDir>,
    path: Option<std::path::PathBuf>,
) {
    let Some(path) = path else {
        return;
    };
    if path.as_os_str().is_empty() {
        return;
    }

    let scan_immediate_subdirs = is_dedicated_firmware_root(&path);
    push_firmware_search_dir(out, Some(path.clone()));

    if !scan_immediate_subdirs {
        return;
    }

    let Ok(entries) = std::fs::read_dir(&path) else {
        return;
    };
    let mut subdirs = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let Ok(file_type) = entry.file_type() else {
                return None;
            };
            file_type.is_dir().then(|| entry.path())
        })
        .collect::<Vec<_>>();
    subdirs.sort();

    for subdir in subdirs {
        push_firmware_search_dir(out, Some(subdir));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn is_dedicated_firmware_root(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    FIRMWARE_ROOT_DIR_NAMES
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
}

#[cfg(not(target_arch = "wasm32"))]
fn push_firmware_search_dir(out: &mut Vec<FirmwareSearchDir>, path: Option<std::path::PathBuf>) {
    let Some(path) = path else {
        return;
    };
    if path.as_os_str().is_empty() {
        return;
    }
    if out.iter().any(|existing| existing.path == path) {
        return;
    }
    out.push(FirmwareSearchDir { path });
}

#[cfg(not(target_arch = "wasm32"))]
fn expected_filenames_for_plan(
    catalog: &[zeff_firmware::FirmwareSpec],
    plan: &[zeff_firmware::FirmwareRequest],
) -> std::collections::BTreeSet<String> {
    plan.iter()
        .filter_map(|request| {
            catalog
                .iter()
                .find(|spec| spec.id == request.id.as_ref())
                .map(|spec| (request, spec))
        })
        .flat_map(|(_request, spec)| spec.variants.iter())
        .flat_map(|variant| variant.filenames.iter())
        .map(|filename| filename.to_ascii_lowercase())
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn scan_known_firmware_filenames(
    dir: &std::path::Path,
    expected_filenames: &std::collections::BTreeSet<String>,
    catalog: &[zeff_firmware::FirmwareSpec],
) -> zeff_firmware::FirmwareInventory {
    let mut inventory = zeff_firmware::FirmwareInventory::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return inventory;
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let filename = entry.file_name();
        let Some(filename_str) = filename.to_str() else {
            continue;
        };
        if !expected_filenames.contains(&filename_str.to_ascii_lowercase()) {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        inventory.add(zeff_firmware::FirmwareInventoryEntry::from_bytes(
            bytes,
            Some(filename_str.to_owned()),
            catalog,
        ));
    }

    inventory
}

#[cfg(not(target_arch = "wasm32"))]
fn firmware_candidate_summary(candidates: &[zeff_firmware::FirmwareCandidate]) -> String {
    if candidates.is_empty() {
        return String::new();
    }

    let names = candidates
        .iter()
        .filter_map(|candidate| candidate.original_filename.as_deref())
        .collect::<Vec<_>>();
    if names.is_empty() {
        " Candidate files had plausible size but unknown names/hashes.".to_owned()
    } else {
        format!(
            " Candidate file(s) with plausible size but unrecognized hash: {}.",
            names.join(", ")
        )
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires ZEFF_FIRMWARE_TEST_DIR with a retail GBA BIOS"]
    fn configured_retail_gba_bios_resolves_as_external() {
        let root = std::path::PathBuf::from(std::env::var("ZEFF_FIRMWARE_TEST_DIR").unwrap());
        let resolved = resolve_gba_bios_with_manifest(None, &[root], None).unwrap();

        assert_eq!(resolved.bytes.len(), 16_384);
        assert!(matches!(
            resolved.manifest,
            zeff_emu_common::replay::ReplayFirmwareManifest::External {
                ref firmware_id,
                ..
            } if firmware_id == "nintendo.gba.bios"
        ));
    }

    #[test]
    fn injected_inventory_resolves_before_missing_search_roots() {
        let mut inventory = zeff_firmware::FirmwareInventory::new();
        inventory.add(
            zeff_firmware::FirmwareInventoryEntry::from_bytes_with_legacy_digests(
                vec![0; 16_384],
                Some("gba_bios.bin".to_owned()),
                Some("a860e8c0b6d573d191e4ec7db1b1e4f6".to_owned()),
                None,
                zeff_firmware::catalog_specs(),
            ),
        );

        let resolved = resolve_gba_bios_with_manifest(
            Some(&inventory),
            &[std::path::PathBuf::from("does-not-exist")],
            None,
        )
        .unwrap();

        assert_eq!(resolved.bytes.len(), 16_384);
        assert!(matches!(
            resolved.manifest,
            zeff_emu_common::replay::ReplayFirmwareManifest::External {
                ref firmware_id,
                ..
            } if firmware_id == "nintendo.gba.bios"
        ));
    }

    #[test]
    fn empty_injected_inventory_keeps_native_search_fallback() {
        let err = resolve_gba_bios_with_manifest(
            Some(&zeff_firmware::FirmwareInventory::new()),
            &[std::path::PathBuf::from("does-not-exist")],
            None,
        )
        .unwrap_err();

        assert!(err.to_string().contains("does-not-exist"));
    }

    #[test]
    fn content_near_firmware_search_is_nonrecursive_and_limited() {
        let dirs = firmware_search_dirs(
            &[],
            Some(std::path::Path::new(
                "Z:/Android/Roms/FDS/Bubble Bobble.fds",
            )),
        );

        assert!(dirs.contains(&FirmwareSearchDir {
            path: std::path::PathBuf::from("Z:/Android/Roms/FDS"),
        }));
        assert!(dirs.contains(&FirmwareSearchDir {
            path: std::path::PathBuf::from("Z:/Android/Roms/FDS/bios"),
        }));
        assert!(dirs.contains(&FirmwareSearchDir {
            path: std::path::PathBuf::from("Z:/Android/Roms/FDS/BIOS Files"),
        }));
        assert!(dirs.contains(&FirmwareSearchDir {
            path: std::path::PathBuf::from("Z:/Android/Roms/bios"),
        }));
        assert!(
            !dirs
                .iter()
                .any(|dir| dir.path == std::path::Path::new("Z:/Android/Roms"))
        );
    }

    #[test]
    fn configured_bios_root_searches_immediate_system_subdirs() {
        let root = std::env::temp_dir()
            .join(format!(
                "zeff_configured_firmware_root_{}",
                std::process::id()
            ))
            .join("BIOS");
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
        std::fs::create_dir_all(root.join("FDS")).expect("FDS firmware dir should be created");
        std::fs::create_dir_all(root.join("PlayStation"))
            .expect("PlayStation firmware dir should be created");

        let dirs = firmware_search_dirs(std::slice::from_ref(&root), None);
        let _ = std::fs::remove_dir_all(root.parent().unwrap());

        assert!(dirs.contains(&FirmwareSearchDir { path: root.clone() }));
        assert!(dirs.contains(&FirmwareSearchDir {
            path: root.join("FDS"),
        }));
        assert!(dirs.contains(&FirmwareSearchDir {
            path: root.join("PlayStation"),
        }));
    }

    #[test]
    fn configured_non_firmware_root_does_not_scan_immediate_subdirs() {
        let root = std::env::temp_dir()
            .join(format!("zeff_configured_rom_root_{}", std::process::id()))
            .join("Roms");
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
        std::fs::create_dir_all(root.join("FDS")).expect("FDS ROM dir should be created");

        let dirs = firmware_search_dirs(std::slice::from_ref(&root), None);
        let _ = std::fs::remove_dir_all(root.parent().unwrap());

        assert!(dirs.contains(&FirmwareSearchDir { path: root.clone() }));
        assert!(!dirs.contains(&FirmwareSearchDir {
            path: root.join("FDS"),
        }));
    }

    #[test]
    fn known_filename_scan_ignores_unrelated_rom_files() {
        let root =
            std::env::temp_dir().join(format!("zeff_known_firmware_scan_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("temp dir should be created");
        std::fs::write(root.join("game.fds"), vec![0x55; 65_500]).unwrap();
        std::fs::write(root.join("DISKSYS.ROM"), vec![0xAA; 8_192]).unwrap();

        let catalog = zeff_firmware::catalog_specs();
        let plan = zeff_firmware::firmware_plan_for_famicom_disk_system();
        let expected = expected_filenames_for_plan(catalog, &plan);
        let inventory = scan_known_firmware_filenames(&root, &expected, catalog);
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(inventory.entries().len(), 1);
        assert_eq!(
            inventory.entries()[0].original_filename.as_deref(),
            Some("DISKSYS.ROM")
        );
    }
}
