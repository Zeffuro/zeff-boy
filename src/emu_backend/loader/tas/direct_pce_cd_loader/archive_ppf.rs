use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, ensure};
use zeff_pce_core::hardware::{
    PceArcadeCardMode, PceCartridgeHardware, PceConsoleWiring, PceControllerMode, PceMemoryBaseMode,
};

use super::DirectPceCdTasExecutionLoader;
use crate::emu_backend::loader::tas::direct_pce_cd::PceCdTasProfile;
use crate::emu_backend::loader::{BackendLoadConfig, EmuBackend, has_extension};
use crate::emu_backend::pce_cd::PceCdLoadError;
use crate::emu_backend::pce_cd_archive::{
    PceCdArchivePpfCandidate, PceCdArchivePpfLoad, PceCdArchivePpfPatchIdentity,
    PceCdPackageProgress,
};

impl DirectPceCdTasExecutionLoader {
    pub(super) fn inspect_archive_ppf_candidates(&self) -> Result<Vec<PceCdArchivePpfCandidate>> {
        let cancel = Arc::new(AtomicBool::new(false));
        let candidates = if has_extension(&self.source_path, "7z") {
            let progress = PceCdPackageProgress::default();
            crate::emu_backend::pce_cd_archive::inspect_7z_ppf_candidates_with_archive_identity(
                &self.source_path,
                cancel.as_ref(),
                &progress,
                512,
            )?
        } else if has_extension(&self.source_path, "rar") {
            crate::emu_backend::pce_cd_rar::inspect_rar_ppf_candidates_with_archive_identity(
                &self.source_path,
                Arc::clone(&cancel),
            )?
        } else {
            crate::emu_backend::pce_cd_zip::inspect_zip_ppf_candidates_with_archive_identity(
                &self.source_path,
                cancel.as_ref(),
            )?
        };
        Ok(candidates
            .into_iter()
            .filter(|candidate| !candidate.patches.is_empty())
            .collect())
    }

    pub(super) fn try_load_archive_ppf_backend(&self) -> Result<Option<EmuBackend>> {
        let load = match self.load_archive_ppf() {
            Ok(load) => load,
            Err(PceCdLoadError::NoArchivePpfStack) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        canonical_patch_order(&load)?;
        let archive = has_extension(&self.source_path, "7z");
        let rar = has_extension(&self.source_path, "rar");
        let zip = has_extension(&self.source_path, "zip");
        let selected = self.archive_cue_member.is_some();
        let profile = PceCdTasProfile::from_runtime_flags(
            (false, false, false, archive, rar, zip),
            true,
            (archive && selected, rar && selected, zip && selected),
            (false, false),
            PceControllerMode::TwoButton,
        )
        .context("archive PPF source describes an invalid execution profile")?;
        let patch_identities = load.patch_identities();
        let parts = identity_parts(&patch_identities);
        let source = profile
            .archive_ppf_source_identity(
                load.archive_identity.source_sha256,
                load.archive_identity.source_len,
                load.archive_identity.cue_member_path_sha256,
                &parts,
            )
            .context("archive PPF source omitted its execution profile")?;
        #[cfg(test)]
        let (system_card_override, system_card_sha256_override) =
            if self.system_card_override.is_some() {
                (self.system_card_override, self.system_card_sha256_override)
            } else {
                super::test_registry::sole_system_card()
            };
        let identity = load.archive_identity;
        let config = BackendLoadConfig {
            sample_rate: Some(48_000),
            apply_mods: false,
            initial_input: None,
            pce_console_wiring: Some(PceConsoleWiring::PcEngine),
            pce_cartridge_hardware: Some(PceCartridgeHardware::Base),
            pce_cd_tas_source_media: Some((source.0, identity.source_len, profile.sync_config().0)),
            pce_cd_tas_archive_cue: archive.then_some(identity),
            pce_cd_tas_rar_cue: rar.then_some(identity),
            pce_cd_tas_zip_cue: zip.then_some(identity),
            pce_controller_mode: PceControllerMode::TwoButton,
            pce_memory_base_mode: PceMemoryBaseMode::Disabled,
            pce_arcade_card_mode: PceArcadeCardMode::Disabled,
            pce_load_battery_bram: false,
            firmware_search_dirs: self.firmware_search_dirs.clone(),
            #[cfg(test)]
            pce_cd_system_card_override: system_card_override,
            #[cfg(test)]
            pce_cd_system_card_sha256_override: system_card_sha256_override,
            ..BackendLoadConfig::default()
        };
        Ok(Some(
            crate::emu_backend::loader::pce_cd::finish_preloaded_archive_ppf_backend(
                &self.source_path,
                load,
                &config,
            )?
            .backend,
        ))
    }

    fn load_archive_ppf(&self) -> Result<PceCdArchivePpfLoad, PceCdLoadError> {
        if has_extension(&self.source_path, "7z") {
            let cancel = AtomicBool::new(false);
            let progress = PceCdPackageProgress::default();
            if let Some(selected) = &self.archive_cue_member {
                crate::emu_backend::pce_cd_archive::load_7z_selected_cue_with_control_and_archive_ppf(
                    &self.source_path,
                    selected,
                    &cancel,
                    &progress,
                    512,
                )
            } else {
                crate::emu_backend::pce_cd_archive::load_7z_cue_with_control_and_archive_ppf(
                    &self.source_path,
                    &cancel,
                    &progress,
                    512,
                )
            }
        } else {
            let cancel = Arc::new(AtomicBool::new(false));
            let progress = Arc::new(PceCdPackageProgress::default());
            if has_extension(&self.source_path, "rar") {
                if let Some(selected) = &self.archive_cue_member {
                    crate::emu_backend::pce_cd_rar::load_rar_selected_cue_with_control_and_archive_ppf(
                        &self.source_path, selected, cancel, progress,
                    )
                } else {
                    crate::emu_backend::pce_cd_rar::load_rar_cue_with_control_and_archive_ppf(
                        &self.source_path,
                        cancel,
                        progress,
                    )
                }
            } else if let Some(selected) = &self.archive_cue_member {
                crate::emu_backend::pce_cd_zip::load_zip_selected_cue_with_control_and_archive_ppf(
                    &self.source_path,
                    selected,
                    cancel,
                    progress,
                )
            } else {
                crate::emu_backend::pce_cd_zip::load_zip_cue_with_control_and_archive_ppf(
                    &self.source_path,
                    cancel,
                    progress,
                )
            }
        }
    }
}

pub(super) fn identity_parts(
    patches: &[PceCdArchivePpfPatchIdentity],
) -> Vec<(&str, usize, [u8; 32])> {
    patches
        .iter()
        .map(|patch| (patch.member_path.as_str(), patch.len, patch.sha256))
        .collect()
}

fn canonical_patch_order(load: &PceCdArchivePpfLoad) -> Result<()> {
    let identities = load.patch_identities();
    ensure!(!identities.is_empty(), "archive PPF stack is empty");
    for (index, patch) in identities.iter().enumerate() {
        ensure!(
            patch.member_path.rsplit('/').next() == Some(&format!("{:04}.ppf", index + 1)),
            "archive PPF stack is not in canonical numeric order"
        );
    }
    Ok(())
}
