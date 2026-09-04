use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use zeff_emu_common::replay::ReplayStartMetadata;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

use super::direct_gb::{
    MAX_DIRECT_GB_ROM_BYTES, direct_gb_battery_tas_sync_config_sha256, direct_gb_tas_devices,
    direct_gb_tas_firmware, direct_gb_tas_sync_config_sha256, gb_battery_sram, read_direct_gb_rom,
    validate_direct_gb_rom, validate_direct_gb_tas_branch_scope,
    validate_direct_gb_tas_project_identity, validate_direct_gb_tas_runtime_inner,
    validate_strict_gb_start_state, zip_gb_battery_tas_sync_config_sha256,
    zip_gb_tas_sync_config_sha256,
};
use super::gb_rtc::{
    GB_TAS_RTC_EPOCH_UNIX_SECONDS, GbTasRtcHardware, canonicalize_gb_tas_start_state,
    gb_rtc_external_identities, gb_rtc_sync_config_sha256,
};
use super::media::reject_embedded_zip_sram;
use super::{
    BackendLoadConfig, EmuBackend, TasDigest, TasEditorExecutionEngine, TasEditorExecutionProvider,
    TasExecutionSession, TasExternalIdentity, TasInitialBranch, TasProject, TasProjectIdentity,
    has_extension, publish_new_project,
};

const MAX_GB_ZIP_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct DirectGbTasExecutionLoader {
    source_path: PathBuf,
    rom_path: Option<PathBuf>,
    firmware_search_dirs: Vec<PathBuf>,
}

#[derive(Clone, Copy)]
pub(super) struct GbTasMediaIdentity {
    source_media_sha256: TasDigest,
    source_media_len: usize,
    sync_config_sha256: TasDigest,
}

impl DirectGbTasExecutionLoader {
    pub(crate) fn new(source_path: PathBuf, firmware_search_dirs: Vec<PathBuf>) -> Self {
        Self {
            source_path,
            rom_path: None,
            firmware_search_dirs,
        }
    }

    pub(crate) fn new_zip(
        source_path: PathBuf,
        rom_path: Option<PathBuf>,
        firmware_search_dirs: Vec<PathBuf>,
    ) -> Self {
        Self {
            source_path,
            rom_path,
            firmware_search_dirs,
        }
    }

    pub(crate) fn new_zip_for_project(
        source_path: PathBuf,
        firmware_search_dirs: Vec<PathBuf>,
        project: &TasProject,
    ) -> Result<Self> {
        validate_direct_gb_tas_project_identity(project)?;
        let inspection = crate::rom_archive::inspect_bounded_zip_members(
            &source_path,
            "gb",
            MAX_GB_ZIP_BYTES,
            MAX_DIRECT_GB_ROM_BYTES,
        )?;
        ensure!(
            TasDigest(inspection.archive_sha256) == project.identity().source_media_sha256,
            "GB ZIP archive does not match the TAS project"
        );
        let matches = inspection
            .entries
            .into_iter()
            .filter(|entry| {
                if project.identity().rtc_state != TasExternalIdentity::Absent {
                    [0, 8 * 1024, 32 * 1024].into_iter().any(|ram_len| {
                        gb_rtc_sync_config_sha256(
                            GbTasRtcHardware::Dmg,
                            ram_len,
                            Some(&entry.member_name),
                        ) == project.identity().sync_config_sha256
                    })
                } else {
                    let sync = match project.identity().persistent_state {
                        TasExternalIdentity::Absent => {
                            zip_gb_tas_sync_config_sha256(&entry.member_name)
                        }
                        TasExternalIdentity::ExternalSha256(_) => {
                            zip_gb_battery_tas_sync_config_sha256(&entry.member_name)
                        }
                    };
                    sync == project.identity().sync_config_sha256
                }
            })
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "GB ZIP member does not match the TAS project"
        );
        Ok(Self::new_zip(
            source_path,
            Some(matches[0].rom_path.clone()),
            firmware_search_dirs,
        ))
    }

    pub(crate) fn validate_project_branch_scope(
        project: &TasProject,
        branch_id: &str,
    ) -> Result<()> {
        validate_direct_gb_tas_branch_scope(project, branch_id)
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let (backend, media) = self.load_creation_backend()?;
        let start_state = canonicalize_gb_tas_start_state(backend.encode_state_bytes()?);
        let identity = self.identity(&backend, media, &start_state)?;
        let project_id = format!("gb-{}", identity.source_media_sha256.to_hex());
        TasProject::new(
            project_id,
            identity,
            start_state,
            ReplayStartMetadata::default(),
            TasInitialBranch {
                id: "main".to_owned(),
                name: "Main".to_owned(),
                frame_count: 1,
                input_spans: Vec::new(),
                events: Vec::new(),
            },
            BTreeMap::new(),
        )
    }

    pub(crate) fn create_project_file(&self, path: &Path) -> Result<TasProject> {
        ensure!(
            TasProject::is_project_path(path),
            "TAS projects must use the .ztas extension"
        );
        let project = self.create_project()?;
        publish_new_project(path, &project)?;
        Ok(project)
    }

    pub(crate) fn replace_project_file(&self, path: &Path) -> Result<TasProject> {
        ensure!(
            TasProject::is_project_path(path),
            "TAS projects must use the .ztas extension"
        );
        ensure!(path.exists(), "TAS project destination does not exist");
        let project = self.create_project()?;
        project.save_atomic(path).with_context(|| {
            format!(
                "failed to atomically replace TAS project {}",
                path.display()
            )
        })?;
        Ok(project)
    }

    pub(crate) fn load_session(&self, start_state: &[u8]) -> Result<TasExecutionSession> {
        let (mut backend, media) = self.load_fresh_backend()?;
        let fresh_start_state = backend.encode_state_bytes()?;
        let battery_backed = gb_battery_sram(&backend)?.is_some();
        if !battery_backed {
            ensure!(
                fresh_start_state == start_state,
                "GB TAS starting state does not match the fresh direct-ROM baseline"
            );
        }
        let gb = match &mut backend {
            EmuBackend::Gb(gb) => gb,
            _ => bail!("TAS execution profile requires a Game Boy backend"),
        };
        let projection = zeff_gb_core::save_state::validate_and_load_current_native_tas_state(
            &mut gb.emu,
            start_state,
        )
        .context("failed to restore strict GB TAS starting state")?;
        ensure!(
            projection.frame_count == 0
                && projection.lcd_framebuffer.len() == 160 * 144 * 4
                && projection.lcd_framebuffer.as_ref() == backend.framebuffer(),
            "GB TAS starting state does not restore the fresh baseline frame"
        );
        let identity = self.identity(&backend, media, start_state)?;
        Ok(TasExecutionSession::new(backend, identity))
    }

    pub(crate) fn load_editor_engine(
        &self,
        project: &TasProject,
    ) -> Result<TasEditorExecutionEngine> {
        for branch in project.branches() {
            Self::validate_project_branch_scope(project, branch.id()).with_context(|| {
                format!(
                    "TAS branch {:?} is outside the direct Game Boy editor execution profile",
                    branch.id()
                )
            })?;
        }
        let session = self.load_session(project.start_state())?;
        TasEditorExecutionEngine::attach(project, session, validate_direct_gb_tas_branch_scope)
    }

    fn load_creation_backend(&self) -> Result<(EmuBackend, GbTasMediaIdentity)> {
        self.load_backend(true)
    }

    pub(super) fn load_fresh_backend(&self) -> Result<(EmuBackend, GbTasMediaIdentity)> {
        self.load_backend(false)
    }

    fn load_backend(&self, load_battery_sram: bool) -> Result<(EmuBackend, GbTasMediaIdentity)> {
        let (rom_path, source_bytes, media, load_battery_sram, rtc) = if has_extension(
            &self.source_path,
            "gb",
        ) {
            let source_bytes = read_direct_gb_rom(&self.source_path)?;
            let header = validate_direct_gb_rom(&source_bytes)?;
            let rtc = header.cartridge_type.is_mbc3_with_rtc();
            let sync_config_sha256 = if rtc {
                gb_rtc_sync_config_sha256(GbTasRtcHardware::Dmg, header.ram_size.size_bytes(), None)
            } else if header.cartridge_type.is_battery_backed() {
                direct_gb_battery_tas_sync_config_sha256()
            } else {
                direct_gb_tas_sync_config_sha256()
            };
            let media = GbTasMediaIdentity {
                source_media_sha256: TasDigest::from_bytes(&source_bytes),
                source_media_len: source_bytes.len(),
                sync_config_sha256,
            };
            (
                self.source_path.clone(),
                source_bytes,
                media,
                load_battery_sram,
                rtc,
            )
        } else if has_extension(&self.source_path, "zip") {
            let selected = crate::rom_archive::extract_bounded_zip_member(
                &self.source_path,
                self.rom_path.as_deref(),
                "gb",
                MAX_GB_ZIP_BYTES,
                MAX_DIRECT_GB_ROM_BYTES,
            )?;
            let header = validate_direct_gb_rom(&selected.bytes)?;
            let battery_backed = header.cartridge_type.is_battery_backed();
            let rtc = header.cartridge_type.is_mbc3_with_rtc();
            if battery_backed {
                reject_embedded_zip_sram(
                    &self.source_path,
                    MAX_GB_ZIP_BYTES,
                    MAX_DIRECT_GB_ROM_BYTES,
                    "GB ZIP TAS execution does not import embedded SRAM; use an adjacent .sav file",
                )?;
            }
            let media = GbTasMediaIdentity {
                source_media_sha256: TasDigest(selected.archive_sha256),
                source_media_len: selected.archive_len,
                sync_config_sha256: if rtc {
                    gb_rtc_sync_config_sha256(
                        GbTasRtcHardware::Dmg,
                        header.ram_size.size_bytes(),
                        Some(&selected.member_name),
                    )
                } else if battery_backed {
                    zip_gb_battery_tas_sync_config_sha256(&selected.member_name)
                } else {
                    zip_gb_tas_sync_config_sha256(&selected.member_name)
                },
            };
            (
                selected.rom_path,
                selected.bytes,
                media,
                load_battery_sram,
                rtc,
            )
        } else {
            bail!("GB TAS execution requires a direct .gb file or selected ZIP member");
        };
        let backend = super::load_backend_from_rom_source(
            super::ActiveSystem::GameBoy,
            &self.source_path,
            &rom_path,
            Some(source_bytes),
            BackendLoadConfig {
                gb_hardware_mode_preference: HardwareModePreference::ForceDmg,
                sample_rate: None,
                apply_mods: false,
                initial_input: None,
                gb_load_battery_sram: load_battery_sram,
                gb_rtc_time_override: rtc.then_some(GB_TAS_RTC_EPOCH_UNIX_SECONDS),
                gb_tas_source_media: Some((
                    media.source_media_sha256.0,
                    media.source_media_len,
                    media.sync_config_sha256.0,
                )),
                firmware_search_dirs: self.firmware_search_dirs.clone(),
                gb_use_external_boot_rom: false,
                ..BackendLoadConfig::default()
            },
        )?
        .backend;
        let start_state = canonicalize_gb_tas_start_state(backend.encode_state_bytes()?);
        self.identity(&backend, media, &start_state)?;
        Ok((backend, media))
    }

    fn identity(
        &self,
        backend: &EmuBackend,
        media: GbTasMediaIdentity,
        start_state: &[u8],
    ) -> Result<TasProjectIdentity> {
        if has_extension(&self.source_path, "gb") {
            let source_bytes = read_direct_gb_rom(&self.source_path)?;
            ensure!(
                media.source_media_sha256 == TasDigest::from_bytes(&source_bytes),
                "Game Boy source changed while constructing TAS identity"
            );
            return direct_gb_tas_identity(backend, &source_bytes, start_state);
        }
        let selected = crate::rom_archive::extract_bounded_zip_member(
            &self.source_path,
            self.rom_path.as_deref(),
            "gb",
            MAX_GB_ZIP_BYTES,
            MAX_DIRECT_GB_ROM_BYTES,
        )?;
        ensure!(
            media.source_media_sha256 == TasDigest(selected.archive_sha256)
                && media.sync_config_sha256
                    == if backend
                        .gb_tas_load_provenance()
                        .is_some_and(|provenance| provenance.cartridge_type.is_mbc3_with_rtc())
                    {
                        gb_rtc_sync_config_sha256(
                            GbTasRtcHardware::Dmg,
                            backend.gb().unwrap().emu.header().ram_size.size_bytes(),
                            Some(&selected.member_name),
                        )
                    } else if gb_battery_sram(backend)?.is_some() {
                        zip_gb_battery_tas_sync_config_sha256(&selected.member_name)
                    } else {
                        zip_gb_tas_sync_config_sha256(&selected.member_name)
                    }
                && TasDigest::from_bytes(&selected.bytes) == TasDigest(backend.rom_hash()),
            "GB ZIP or selected member changed while constructing TAS identity"
        );
        zip_gb_tas_identity(
            backend,
            selected.archive_sha256,
            &selected.member_name,
            start_state,
        )
    }
}

impl TasEditorExecutionProvider for DirectGbTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectGbTasExecutionLoader::load_editor_engine(self, project)
    }
}

pub(crate) fn direct_gb_tas_identity(
    backend: &EmuBackend,
    source_bytes: &[u8],
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    validate_direct_gb_rom(source_bytes)?;
    let rtc = backend
        .gb_tas_load_provenance()
        .is_some_and(|provenance| provenance.cartridge_type.is_mbc3_with_rtc());
    let (persistent_state, rtc_state) = if rtc {
        gb_rtc_external_identities(backend)?
    } else {
        (
            match gb_battery_sram(backend)? {
                Some(sram) => TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&sram)),
                None => TasExternalIdentity::Absent,
            },
            TasExternalIdentity::Absent,
        )
    };
    let sync_config_sha256 = if rtc {
        gb_rtc_sync_config_sha256(
            GbTasRtcHardware::Dmg,
            backend.gb().unwrap().emu.header().ram_size.size_bytes(),
            None,
        )
    } else {
        match persistent_state {
            TasExternalIdentity::Absent => direct_gb_tas_sync_config_sha256(),
            TasExternalIdentity::ExternalSha256(_) => direct_gb_battery_tas_sync_config_sha256(),
        }
    };
    let identity = gb_tas_identity(
        backend,
        TasDigest::from_bytes(source_bytes),
        sync_config_sha256,
        persistent_state,
        rtc_state,
        start_state,
    )?;
    ensure!(
        identity.source_media_sha256 == identity.effective_media_sha256,
        "direct Game Boy loader changed media bytes"
    );
    Ok(identity)
}

fn zip_gb_tas_identity(
    backend: &EmuBackend,
    archive_sha256: [u8; 32],
    member_name: &str,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let rtc = backend
        .gb_tas_load_provenance()
        .is_some_and(|provenance| provenance.cartridge_type.is_mbc3_with_rtc());
    let (persistent_state, rtc_state) = if rtc {
        gb_rtc_external_identities(backend)?
    } else {
        (
            match gb_battery_sram(backend)? {
                Some(sram) => TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&sram)),
                None => TasExternalIdentity::Absent,
            },
            TasExternalIdentity::Absent,
        )
    };
    let sync_config_sha256 = if rtc {
        gb_rtc_sync_config_sha256(
            GbTasRtcHardware::Dmg,
            backend.gb().unwrap().emu.header().ram_size.size_bytes(),
            Some(member_name),
        )
    } else {
        match persistent_state {
            TasExternalIdentity::Absent => zip_gb_tas_sync_config_sha256(member_name),
            TasExternalIdentity::ExternalSha256(_) => {
                zip_gb_battery_tas_sync_config_sha256(member_name)
            }
        }
    };
    gb_tas_identity(
        backend,
        TasDigest(archive_sha256),
        sync_config_sha256,
        persistent_state,
        rtc_state,
        start_state,
    )
}

fn gb_tas_identity(
    backend: &EmuBackend,
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    persistent_state: TasExternalIdentity,
    rtc_state: TasExternalIdentity,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    validate_direct_gb_tas_runtime_inner(backend, false, true)?;
    let metadata = backend.replay_metadata();
    let system = metadata
        .system
        .context("Game Boy backend omitted its system identity")?;
    let core_family = metadata
        .core_family
        .context("Game Boy backend omitted its core-family identity")?;
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .context("Game Boy backend omitted its effective media identity")?,
    );
    validate_strict_gb_start_state(start_state)?;
    let provenance = backend
        .gb_tas_load_provenance()
        .context("Game Boy backend omitted load provenance")?;
    ensure!(
        TasDigest(provenance.load.tas_source_media_sha256) == source_media_sha256
            && TasDigest(provenance.load.tas_sync_config_sha256) == sync_config_sha256,
        "Game Boy source provenance differs from the TAS media profile"
    );
    Ok(TasProjectIdentity {
        system,
        core_family,
        determinism_abi: zeff_gb_core::save_state::TAS_DETERMINISM_ABI_ID.to_owned(),
        source_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: direct_gb_tas_firmware(),
        devices: direct_gb_tas_devices(),
        sync_config_sha256,
        persistent_state,
        rtc_state,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id: zeff_gb_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID
            .to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    })
}
