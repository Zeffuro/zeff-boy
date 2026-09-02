use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::platform;

pub(crate) mod recovery_state;
#[cfg(not(target_arch = "wasm32"))]
mod sram_recovery;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use sram_recovery::{
    RecoverySession as SramRecoverySession, SavePublicationOutcome, SaveRecoveryIdentity,
    SaveTargetBaseline,
};

#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub(crate) struct SramRecoverySession;

#[cfg(target_arch = "wasm32")]
impl SramRecoverySession {
    pub(crate) fn begin(
        &mut self,
        _primary_path: &Path,
        _system_subdir: &str,
        _media_identity: [u8; 32],
        _component: &str,
    ) {
    }
}

pub(crate) const SRAM_COMPONENT: &str = "sram";
pub(crate) const GB_RTC_COMPONENT: &str = "gb-rtc";
pub(crate) const GBA_BACKUP_COMPONENT: &str = "gba-backup";
pub(crate) const GBA_RTC_COMPONENT: &str = "gba-rtc";
pub(crate) const WS_BACKUP_COMPONENT: &str = "ws-backup";
pub(crate) const WS_RTC_COMPONENT: &str = "ws-rtc";

pub(crate) fn slot_path(
    system_subdir: &str,
    state_ext: &str,
    rom_hash: [u8; 32],
    slot: u8,
) -> anyhow::Result<PathBuf> {
    if slot > 9 {
        anyhow::bail!("invalid save-state slot {slot} (must be 0–9)");
    }
    let hash_hex = hex_hash(&rom_hash);
    let mut path = platform::save_dir(system_subdir);
    path.push(format!("{hash_hex}_slot{slot}.{state_ext}"));
    Ok(path)
}

pub(crate) fn write_state_bytes_to_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    platform::write_save_data(path, bytes)
}

pub(crate) fn backup_state_path(path: &Path) -> PathBuf {
    let backup_ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!("{ext}.bak"))
        .unwrap_or_else(|| "bak".to_string());
    path.with_extension(backup_ext)
}

pub(crate) fn write_state_bytes_to_file_with_backup(
    path: &Path,
    bytes: &[u8],
) -> anyhow::Result<bool> {
    let backup_created = if let Some(previous) = platform::read_save_data(path)? {
        let backup_path = backup_state_path(path);
        platform::write_save_data(&backup_path, &previous).with_context(|| {
            format!(
                "failed to back up existing save state from {} to {}",
                path.display(),
                backup_path.display()
            )
        })?;
        true
    } else {
        false
    };

    write_state_bytes_to_file(path, bytes)?;
    Ok(backup_created)
}

pub(crate) fn restore_state_file_backup(path: &Path) -> anyhow::Result<()> {
    let backup_path = backup_state_path(path);
    let bytes = platform::read_save_data(&backup_path)?
        .with_context(|| format!("no save-state backup exists for {}", path.display()))?;
    anyhow::ensure!(
        platform::save_data_exists(path),
        "save state no longer exists: {}",
        path.display()
    );
    write_state_bytes_to_file_with_backup(path, &bytes)?;
    Ok(())
}

fn hex_hash(hash: &[u8; 32]) -> String {
    const_hex::encode(hash)
}

pub(crate) fn sram_path_for_rom(rom_path: &Path) -> PathBuf {
    for ancestor in rom_path.ancestors() {
        if ancestor
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(is_archive_extension)
        {
            return ancestor.with_extension("sav");
        }
    }
    if let Some(archive_path) = backslash_or_slash_archive_ancestor(rom_path) {
        return archive_path.with_extension("sav");
    }
    rom_path.with_extension("sav")
}

fn backslash_or_slash_archive_ancestor(path: &Path) -> Option<PathBuf> {
    let text = path.as_os_str().to_string_lossy();
    let mut component_start = 0;
    for (index, ch) in text.char_indices() {
        if !matches!(ch, '\\' | '/') {
            continue;
        }
        if component_has_archive_extension(&text[component_start..index]) {
            return Some(PathBuf::from(&text[..index]));
        }
        component_start = index + ch.len_utf8();
    }
    component_has_archive_extension(&text[component_start..]).then(|| PathBuf::from(text.as_ref()))
}

fn component_has_archive_extension(component: &str) -> bool {
    Path::new(component)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(is_archive_extension)
}

fn is_archive_extension(extension: &str) -> bool {
    extension.eq_ignore_ascii_case("zip")
        || extension.eq_ignore_ascii_case("7z")
        || extension.eq_ignore_ascii_case("rar")
}

pub(crate) fn flush_battery_sram(
    recovery_session: &mut SramRecoverySession,
    rom_path: &Path,
    system_subdir: &str,
    media_identity: [u8; 32],
    sram_bytes: Option<Vec<u8>>,
) -> anyhow::Result<Option<String>> {
    let Some(bytes) = sram_bytes else {
        return Ok(None);
    };
    let save_path = sram_path_for_rom(rom_path);
    write_recoverable_sram_file(
        recovery_session,
        &save_path,
        system_subdir,
        media_identity,
        SRAM_COMPONENT,
        &bytes,
    )?;
    Ok(Some(save_path.display().to_string()))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn battery_sram_baseline(rom_path: &Path) -> anyhow::Result<SaveTargetBaseline> {
    sram_recovery::capture_baseline(&sram_path_for_rom(rom_path))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn publish_battery_sram_if_unchanged(
    recovery_session: &mut SramRecoverySession,
    rom_path: &Path,
    system_subdir: &str,
    media_identity: [u8; 32],
    expected: SaveTargetBaseline,
    bytes: &[u8],
) -> (String, SavePublicationOutcome) {
    let (path, outcome, _) = publish_battery_sram_if_unchanged_with_receipt(
        recovery_session,
        rom_path,
        system_subdir,
        media_identity,
        expected,
        bytes,
    );
    (path, outcome)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn publish_battery_sram_if_unchanged_with_receipt(
    recovery_session: &mut SramRecoverySession,
    rom_path: &Path,
    system_subdir: &str,
    media_identity: [u8; 32],
    expected: SaveTargetBaseline,
    bytes: &[u8],
) -> (
    String,
    SavePublicationOutcome,
    recovery_state::BatteryPublicationReceipt,
) {
    let save_path = sram_path_for_rom(rom_path);
    let receipt =
        recovery_state::BatteryPublicationReceipt::from_components(&[(SRAM_COMPONENT, bytes)]);
    let outcome = recovery_session.write_if_unchanged(
        &save_path,
        SaveRecoveryIdentity {
            system_subdir,
            media_identity,
            component: SRAM_COMPONENT,
        },
        expected,
        bytes,
    );
    (save_path.display().to_string(), outcome, receipt)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn publish_battery_aggregate_if_unchanged(
    recovery_session: &mut SramRecoverySession,
    rom_path: &Path,
    identity: SaveRecoveryIdentity<'_>,
    expected: SaveTargetBaseline,
    bytes: &[u8],
    receipt: recovery_state::BatteryPublicationReceipt,
) -> (
    String,
    SavePublicationOutcome,
    recovery_state::BatteryPublicationReceipt,
) {
    let save_path = sram_path_for_rom(rom_path);
    let outcome = recovery_session.write_if_unchanged(&save_path, identity, expected, bytes);
    (save_path.display().to_string(), outcome, receipt)
}

pub(crate) fn aggregate_battery_receipt(
    bytes: &[u8],
    backup_len: usize,
    backup_component: &'static str,
    rtc_component: &'static str,
) -> Option<recovery_state::BatteryPublicationReceipt> {
    let rtc = bytes.get(backup_len..)?;
    if rtc.is_empty() {
        return None;
    }
    let components = if backup_len == 0 {
        vec![(rtc_component, rtc)]
    } else {
        vec![
            (backup_component, &bytes[..backup_len]),
            (rtc_component, rtc),
        ]
    };
    Some(recovery_state::BatteryPublicationReceipt::from_components(
        &components,
    ))
}

pub(crate) fn begin_battery_sram_session(
    recovery_session: &mut SramRecoverySession,
    rom_path: &Path,
    system_subdir: &str,
    media_identity: [u8; 32],
) {
    recovery_session.begin(
        &sram_path_for_rom(rom_path),
        system_subdir,
        media_identity,
        SRAM_COMPONENT,
    );
}

pub(crate) fn battery_sram_session(
    rom_path: &Path,
    system_subdir: &str,
    media_identity: [u8; 32],
) -> SramRecoverySession {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    let mut session = SramRecoverySession::authoritative_only();
    #[cfg(all(not(test), not(target_arch = "wasm32")))]
    let mut session = SramRecoverySession::default();
    #[cfg(target_arch = "wasm32")]
    let mut session = SramRecoverySession;
    begin_battery_sram_session(&mut session, rom_path, system_subdir, media_identity);
    session
}

pub(crate) fn write_recoverable_sram_file(
    recovery_session: &mut SramRecoverySession,
    path: &Path,
    system_subdir: &str,
    media_identity: [u8; 32],
    component: &str,
    bytes: &[u8],
) -> anyhow::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        recovery_session.write(path, system_subdir, media_identity, component, bytes)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (recovery_session, path);
        platform::write_sram_data(system_subdir, media_identity, component, bytes)
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) struct BrowserBatterySramRequest<'a> {
    pub(crate) rom_path: &'a Path,
    pub(crate) system_subdir: &'a str,
    pub(crate) media_identity: [u8; 32],
    pub(crate) component: &'a str,
    pub(crate) system_label: &'a str,
    pub(crate) has_battery: bool,
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn try_load_browser_battery_sram(
    request: BrowserBatterySramRequest<'_>,
    load_fn: impl FnOnce(&[u8]) -> anyhow::Result<()>,
) -> anyhow::Result<Option<String>> {
    if !request.has_battery {
        return Ok(None);
    }
    let save_path = sram_path_for_rom(request.rom_path);
    let Some(bytes) = platform::read_sram_data(
        &save_path,
        request.system_subdir,
        request.media_identity,
        request.component,
    )
    .with_context(|| {
        format!(
            "failed to read {} save {}",
            request.system_label,
            save_path.display()
        )
    })?
    else {
        return Ok(None);
    };
    load_fn(&bytes)?;
    Ok(Some(save_path.display().to_string()))
}

pub(crate) fn try_load_battery_sram(
    rom_path: &Path,
    system_label: &str,
    has_battery: bool,
    load_fn: impl FnOnce(&[u8]) -> anyhow::Result<()>,
) -> anyhow::Result<Option<String>> {
    if !has_battery {
        return Ok(None);
    }
    let save_path = sram_path_for_rom(rom_path);
    let Some(bytes) = platform::read_save_data(&save_path)
        .with_context(|| format!("failed to read {system_label} save {}", save_path.display()))?
    else {
        return Ok(None);
    };
    load_fn(&bytes)?;
    Ok(Some(save_path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir(PathBuf);

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn test_dir() -> TestDir {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "zeff-boy-state-backup-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        TestDir(path)
    }

    #[test]
    fn sram_for_regular_rom_uses_rom_stem() {
        assert_eq!(
            sram_path_for_rom(Path::new(r"roms\gba\Game.gba")),
            PathBuf::from(r"roms\gba\Game.sav")
        );
    }

    #[test]
    fn sram_for_zipped_rom_uses_archive_stem() {
        assert_eq!(
            sram_path_for_rom(Path::new(r"roms\gba\Game.zip\Inner.gba")),
            PathBuf::from(r"roms\gba\Game.sav")
        );
    }

    #[test]
    fn sram_for_7z_virtual_member_uses_archive_stem() {
        assert_eq!(
            sram_path_for_rom(Path::new(r"roms\pce-cd\Game.7z\Game.cue")),
            PathBuf::from(r"roms\pce-cd\Game.sav")
        );
    }

    #[test]
    fn sram_for_rar_virtual_member_uses_archive_stem() {
        assert_eq!(
            sram_path_for_rom(Path::new(r"roms\pce-cd\Game.rar\Game.cue")),
            PathBuf::from(r"roms\pce-cd\Game.sav")
        );
    }

    #[test]
    fn absent_sram_does_not_touch_primary_or_recovery() {
        let dir = test_dir();
        let rom_path = dir.0.join("game.gb");
        #[cfg(not(target_arch = "wasm32"))]
        let mut recovery = SramRecoverySession::default();
        #[cfg(target_arch = "wasm32")]
        let mut recovery = SramRecoverySession;

        assert_eq!(
            flush_battery_sram(&mut recovery, &rom_path, "gbc", [0x11; 32], None).unwrap(),
            None
        );
        assert!(!sram_path_for_rom(&rom_path).exists());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn conditional_publication_receipt_matches_exact_rtc_sidecar_bytes() {
        let dir = test_dir();
        let rom_path = dir.0.join("clock.gb");
        let mut recovery = battery_sram_session(&rom_path, "gb", [0x31; 32]);
        let mut bytes = [0u8; 64];
        bytes[40..48].copy_from_slice(&1_725_000_000u64.to_le_bytes());
        bytes[48..56].copy_from_slice(b"ZBRTC001");
        bytes[56..64].copy_from_slice(&2_097_151u64.to_le_bytes());

        let (_, outcome, receipt) = publish_battery_sram_if_unchanged_with_receipt(
            &mut recovery,
            &rom_path,
            "gb",
            [0x31; 32],
            SaveTargetBaseline::Missing,
            &bytes,
        );

        assert!(matches!(outcome, SavePublicationOutcome::PublishedDurable));
        assert_eq!(std::fs::read(sram_path_for_rom(&rom_path)).unwrap(), bytes);
        assert!(receipt.is_consistent());
        assert_eq!(receipt.components.len(), 1);
        assert_eq!(receipt.components[0].name, SRAM_COMPONENT);
        assert_eq!(receipt.components[0].byte_len, 64);
        assert_eq!(
            receipt.components[0].sha256,
            zeff_firmware::sha256_bytes(&bytes)
        );
    }

    #[test]
    fn state_backup_outcome_tracks_only_overwrites() {
        let dir = test_dir();
        let path = dir.0.join("slot.state");
        std::fs::write(backup_state_path(&path), b"stale").unwrap();

        assert!(!write_state_bytes_to_file_with_backup(&path, b"first").unwrap());
        assert!(write_state_bytes_to_file_with_backup(&path, b"second").unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(std::fs::read(backup_state_path(&path)).unwrap(), b"first");
    }

    #[test]
    fn restoring_state_backup_toggles_previous_and_current_bytes() {
        let dir = test_dir();
        let path = dir.0.join("slot.state");
        write_state_bytes_to_file_with_backup(&path, b"first").unwrap();
        write_state_bytes_to_file_with_backup(&path, b"second").unwrap();

        restore_state_file_backup(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        assert_eq!(std::fs::read(backup_state_path(&path)).unwrap(), b"second");

        restore_state_file_backup(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(std::fs::read(backup_state_path(&path)).unwrap(), b"first");
    }
}
