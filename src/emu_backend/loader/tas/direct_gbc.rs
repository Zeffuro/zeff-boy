use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use zeff_emu_common::replay::{ReplayFirmwareManifest, ReplayStartMetadata};
use zeff_gb_core::hardware::GameBoySerialDevice;
use zeff_gb_core::hardware::rom_header::RomHeader;
use zeff_gb_core::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};

use super::direct_gb::{
    DirectGbTasRuntimeWitness, direct_gb_tas_devices, direct_gb_tas_firmware,
    is_supported_direct_gb_tas_cartridge, read_direct_gb_rom, validate_direct_gb_tas_branch_scope,
    validate_strict_gb_start_state,
};
use super::gb_rtc::{
    GB_TAS_RTC_EPOCH_UNIX_SECONDS, GbTasRtcHardware, gb_rtc_external_identities,
    gb_rtc_profile_matches, gb_rtc_sync_config_sha256, validate_gb_rtc_runtime,
};
use super::media::reject_embedded_zip_sram;
use super::{
    BackendLoadConfig, EmuBackend, TasDigest, TasEditorExecutionEngine, TasEditorExecutionProvider,
    TasExecutionSession, TasExternalIdentity, TasInitialBranch, TasProject, TasProjectIdentity,
    has_extension, publish_new_project,
};

const GBC_DIRECT_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gb-rom-only\0hardware=cgb\0media=cgb-exclusive\0boot=internal-post-boot\0serial=disconnected\0mods=disabled\0persistent-state=absent\0initial-input=neutral\0sample-rate=48000\0";
const GBC_BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gb-cartridge\0hardware=cgb\0media=cgb-exclusive\0boot=internal-post-boot\0serial=disconnected\0mods=disabled\0persistent-state=project-owned-sram\0rtc=absent\0sensors=absent\0initial-input=neutral\0sample-rate=48000\0";
const GBC_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gb-zip-member\0hardware=cgb\0media=cgb-exclusive\0boot=internal-post-boot\0serial=disconnected\0mods=disabled\0persistent-state=absent\0rtc=absent\0sensors=absent\0initial-input=neutral\0sample-rate=48000\0member=";
const GBC_ZIP_BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gb-zip-member\0hardware=cgb\0media=cgb-exclusive\0boot=internal-post-boot\0serial=disconnected\0mods=disabled\0persistent-state=project-owned-sram\0rtc=absent\0sensors=absent\0initial-input=neutral\0sample-rate=48000\0member=";
const MAX_GBC_ZIP_BYTES: u64 = 128 * 1024 * 1024;
const MAX_GBC_ROM_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) fn direct_gbc_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(GBC_DIRECT_SYNC_CONFIGURATION)
}

pub(crate) fn direct_gbc_battery_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(GBC_BATTERY_SYNC_CONFIGURATION)
}

pub(crate) fn zip_gbc_tas_sync_config_sha256(member_name: &str) -> TasDigest {
    zip_gbc_tas_sync_config_sha256_for_profile(member_name, false)
}

pub(crate) fn zip_gbc_battery_tas_sync_config_sha256(member_name: &str) -> TasDigest {
    zip_gbc_tas_sync_config_sha256_for_profile(member_name, true)
}

fn zip_gbc_tas_sync_config_sha256_for_profile(member_name: &str, battery: bool) -> TasDigest {
    let configuration = if battery {
        GBC_ZIP_BATTERY_SYNC_CONFIGURATION
    } else {
        GBC_ZIP_SYNC_CONFIGURATION
    };
    let mut bytes = Vec::with_capacity(configuration.len() + member_name.len());
    bytes.extend_from_slice(configuration);
    bytes.extend_from_slice(member_name.as_bytes());
    TasDigest::from_bytes(&bytes)
}

#[derive(Clone, Debug)]
pub(crate) struct DirectGbcTasExecutionLoader {
    source_path: PathBuf,
    rom_path: Option<PathBuf>,
    firmware_search_dirs: Vec<PathBuf>,
}

#[derive(Clone, Copy)]
struct GbcTasMediaIdentity {
    source_media_sha256: TasDigest,
    source_media_len: usize,
    sync_config_sha256: TasDigest,
}

impl DirectGbcTasExecutionLoader {
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
        validate_direct_gbc_tas_project_identity(project)?;
        let inspection = crate::rom_archive::inspect_bounded_zip_members(
            &source_path,
            "gbc",
            MAX_GBC_ZIP_BYTES,
            MAX_GBC_ROM_BYTES,
        )?;
        ensure!(
            TasDigest(inspection.archive_sha256) == project.identity().source_media_sha256,
            "GBC ZIP archive does not match the TAS project"
        );
        let matches = inspection
            .entries
            .into_iter()
            .filter(|entry| {
                if project.identity().rtc_state != TasExternalIdentity::Absent {
                    [0, 8 * 1024, 32 * 1024].into_iter().any(|ram_len| {
                        gb_rtc_sync_config_sha256(
                            GbTasRtcHardware::Cgb,
                            ram_len,
                            Some(&entry.member_name),
                        ) == project.identity().sync_config_sha256
                    })
                } else {
                    let sync = match project.identity().persistent_state {
                        TasExternalIdentity::Absent => {
                            zip_gbc_tas_sync_config_sha256(&entry.member_name)
                        }
                        TasExternalIdentity::ExternalSha256(_) => {
                            zip_gbc_battery_tas_sync_config_sha256(&entry.member_name)
                        }
                    };
                    sync == project.identity().sync_config_sha256
                }
            })
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "GBC ZIP member does not match the TAS project"
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
        validate_direct_gbc_tas_project_identity(project)?;
        validate_direct_gb_tas_branch_scope(project, branch_id)
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let (backend, media) = self.load_creation_backend()?;
        let start_state = backend.encode_state_bytes()?;
        let identity = self.identity(&backend, media, &start_state)?;
        TasProject::new(
            format!("gbc-{}", identity.source_media_sha256.to_hex()),
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
        if gbc_battery_sram(&backend)?.is_none() {
            ensure!(
                backend.encode_state_bytes()? == start_state,
                "GBC TAS starting state does not match the fresh direct-ROM baseline"
            );
        }
        validate_direct_gbc_state_for_backend_inner(&backend, start_state, true, true)?;
        let projection = match &mut backend {
            EmuBackend::Gb(gb) => {
                zeff_gb_core::save_state::validate_and_load_current_native_tas_state(
                    &mut gb.emu,
                    start_state,
                )
                .context("failed to restore strict GBC TAS starting state")?
            }
            _ => bail!("TAS execution profile requires a Game Boy backend"),
        };
        ensure!(
            projection.frame_count == 0
                && projection.lcd_framebuffer.len() == 160 * 144 * 4
                && projection.lcd_framebuffer.as_ref() == backend.framebuffer(),
            "GBC TAS starting state does not restore the fresh baseline frame"
        );
        let identity = self.identity(&backend, media, start_state)?;
        Ok(TasExecutionSession::new(backend, identity))
    }

    pub(crate) fn load_editor_engine(
        &self,
        project: &TasProject,
    ) -> Result<TasEditorExecutionEngine> {
        for branch in project.branches() {
            Self::validate_project_branch_scope(project, branch.id())?;
        }
        let session = self.load_session(project.start_state())?;
        TasEditorExecutionEngine::attach(project, session, |project, branch_id| {
            Self::validate_project_branch_scope(project, branch_id)
        })
    }

    fn load_creation_backend(&self) -> Result<(EmuBackend, GbcTasMediaIdentity)> {
        self.load_backend(true)
    }

    fn load_fresh_backend(&self) -> Result<(EmuBackend, GbcTasMediaIdentity)> {
        self.load_backend(false)
    }

    fn load_backend(&self, load_battery_sram: bool) -> Result<(EmuBackend, GbcTasMediaIdentity)> {
        let (rom_path, source_bytes, media, load_battery_sram, rtc) = if has_extension(
            &self.source_path,
            "gbc",
        ) {
            let source_bytes = read_direct_gb_rom(&self.source_path)?;
            validate_direct_gbc_rom(&source_bytes)?;
            let header = RomHeader::from_rom(&source_bytes)?;
            let rtc = header.cartridge_type.is_mbc3_with_rtc();
            let sync_config_sha256 = if rtc {
                gb_rtc_sync_config_sha256(GbTasRtcHardware::Cgb, header.ram_size.size_bytes(), None)
            } else if header.cartridge_type.is_battery_backed() {
                direct_gbc_battery_tas_sync_config_sha256()
            } else {
                direct_gbc_tas_sync_config_sha256()
            };
            let media = GbcTasMediaIdentity {
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
                "gbc",
                MAX_GBC_ZIP_BYTES,
                MAX_GBC_ROM_BYTES,
            )?;
            validate_direct_gbc_rom(&selected.bytes)?;
            let header = RomHeader::from_rom(&selected.bytes)?;
            let battery_backed = header.cartridge_type.is_battery_backed();
            let rtc = header.cartridge_type.is_mbc3_with_rtc();
            if battery_backed {
                reject_embedded_zip_sram(
                    &self.source_path,
                    MAX_GBC_ZIP_BYTES,
                    MAX_GBC_ROM_BYTES,
                    "GBC ZIP TAS execution does not import embedded SRAM; use an adjacent .sav file",
                )?;
            }
            let media = GbcTasMediaIdentity {
                source_media_sha256: TasDigest(selected.archive_sha256),
                source_media_len: selected.archive_len,
                sync_config_sha256: if rtc {
                    gb_rtc_sync_config_sha256(
                        GbTasRtcHardware::Cgb,
                        header.ram_size.size_bytes(),
                        Some(&selected.member_name),
                    )
                } else if battery_backed {
                    zip_gbc_battery_tas_sync_config_sha256(&selected.member_name)
                } else {
                    zip_gbc_tas_sync_config_sha256(&selected.member_name)
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
            bail!("GBC TAS execution requires a direct .gbc file or selected ZIP member");
        };
        let backend = super::load_backend_from_rom_source(
            super::ActiveSystem::GameBoy,
            &self.source_path,
            &rom_path,
            Some(source_bytes),
            BackendLoadConfig {
                gb_hardware_mode_preference: HardwareModePreference::ForceCgb,
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
        let start_state = backend.encode_state_bytes()?;
        self.identity(&backend, media, &start_state)?;
        Ok((backend, media))
    }

    fn identity(
        &self,
        backend: &EmuBackend,
        media: GbcTasMediaIdentity,
        start_state: &[u8],
    ) -> Result<TasProjectIdentity> {
        if has_extension(&self.source_path, "gbc") {
            let source_bytes = read_direct_gb_rom(&self.source_path)?;
            ensure!(
                media.source_media_sha256 == TasDigest::from_bytes(&source_bytes),
                "GBC source changed while constructing TAS identity"
            );
            return direct_gbc_tas_identity(backend, &source_bytes, start_state);
        }
        let selected = crate::rom_archive::extract_bounded_zip_member(
            &self.source_path,
            self.rom_path.as_deref(),
            "gbc",
            MAX_GBC_ZIP_BYTES,
            MAX_GBC_ROM_BYTES,
        )?;
        ensure!(
            media.source_media_sha256 == TasDigest(selected.archive_sha256)
                && media.sync_config_sha256
                    == if backend
                        .gb_tas_load_provenance()
                        .is_some_and(|provenance| provenance.cartridge_type.is_mbc3_with_rtc())
                    {
                        gb_rtc_sync_config_sha256(
                            GbTasRtcHardware::Cgb,
                            backend.gb().unwrap().emu.header().ram_size.size_bytes(),
                            Some(&selected.member_name),
                        )
                    } else if gbc_battery_sram(backend)?.is_some() {
                        zip_gbc_battery_tas_sync_config_sha256(&selected.member_name)
                    } else {
                        zip_gbc_tas_sync_config_sha256(&selected.member_name)
                    }
                && TasDigest::from_bytes(&selected.bytes) == TasDigest(backend.rom_hash()),
            "GBC ZIP or selected member changed while constructing TAS identity"
        );
        zip_gbc_tas_identity(
            backend,
            selected.archive_sha256,
            &selected.member_name,
            start_state,
        )
    }
}

impl TasEditorExecutionProvider for DirectGbcTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectGbcTasExecutionLoader::load_editor_engine(self, project)
    }
}

pub(crate) fn direct_gbc_tas_identity(
    backend: &EmuBackend,
    source_bytes: &[u8],
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    validate_direct_gbc_rom(source_bytes)?;
    let rtc = backend
        .gb_tas_load_provenance()
        .is_some_and(|provenance| provenance.cartridge_type.is_mbc3_with_rtc());
    let (persistent_state, rtc_state) = if rtc {
        gb_rtc_external_identities(backend)?
    } else {
        (
            match gbc_battery_sram(backend)? {
                Some(sram) => TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&sram)),
                None => TasExternalIdentity::Absent,
            },
            TasExternalIdentity::Absent,
        )
    };
    let sync_config_sha256 = if rtc {
        gb_rtc_sync_config_sha256(
            GbTasRtcHardware::Cgb,
            backend.gb().unwrap().emu.header().ram_size.size_bytes(),
            None,
        )
    } else {
        match persistent_state {
            TasExternalIdentity::Absent => direct_gbc_tas_sync_config_sha256(),
            TasExternalIdentity::ExternalSha256(_) => direct_gbc_battery_tas_sync_config_sha256(),
        }
    };
    let identity = gbc_tas_identity(
        backend,
        TasDigest::from_bytes(source_bytes),
        sync_config_sha256,
        persistent_state,
        rtc_state,
        start_state,
    )?;
    ensure!(
        identity.source_media_sha256 == identity.effective_media_sha256,
        "direct GBC loader changed media bytes"
    );
    Ok(identity)
}

pub(crate) fn zip_gbc_tas_identity(
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
            match gbc_battery_sram(backend)? {
                Some(sram) => TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&sram)),
                None => TasExternalIdentity::Absent,
            },
            TasExternalIdentity::Absent,
        )
    };
    let sync_config_sha256 = if rtc {
        gb_rtc_sync_config_sha256(
            GbTasRtcHardware::Cgb,
            backend.gb().unwrap().emu.header().ram_size.size_bytes(),
            Some(member_name),
        )
    } else {
        match persistent_state {
            TasExternalIdentity::Absent => zip_gbc_tas_sync_config_sha256(member_name),
            TasExternalIdentity::ExternalSha256(_) => {
                zip_gbc_battery_tas_sync_config_sha256(member_name)
            }
        }
    };
    gbc_tas_identity(
        backend,
        TasDigest(archive_sha256),
        sync_config_sha256,
        persistent_state,
        rtc_state,
        start_state,
    )
}

fn gbc_tas_identity(
    backend: &EmuBackend,
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    persistent_state: TasExternalIdentity,
    rtc_state: TasExternalIdentity,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    validate_direct_gbc_tas_runtime_inner(backend, false, true)?;
    validate_direct_gbc_state_for_backend_inner(backend, start_state, true, true)?;
    let metadata = backend.replay_metadata();
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .context("Game Boy backend omitted its effective media identity")?,
    );
    let provenance = backend
        .gb_tas_load_provenance()
        .context("GBC backend omitted load provenance")?;
    ensure!(
        TasDigest(provenance.load.tas_source_media_sha256) == source_media_sha256
            && TasDigest(provenance.load.tas_sync_config_sha256) == sync_config_sha256,
        "GBC source provenance differs from the TAS media profile"
    );
    Ok(TasProjectIdentity {
        system: metadata
            .system
            .context("GBC backend omitted its system identity")?,
        core_family: metadata
            .core_family
            .context("GBC backend omitted its core-family identity")?,
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

pub(crate) fn validate_direct_gbc_tas_project_identity(project: &TasProject) -> Result<()> {
    let identity = project.identity();
    ensure!(
        identity.system == super::ActiveSystem::GameBoy.code()
            && identity.core_family
                == format!("{:?}", zeff_emu_common::system::CoreFamily::GameBoy),
        "TAS project does not identify the native Game Boy core"
    );
    ensure!(
        identity.determinism_abi == zeff_gb_core::save_state::TAS_DETERMINISM_ABI_ID
            && identity.state_format_compatibility_id
                == zeff_gb_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        "TAS project uses an incompatible GBC determinism or state format"
    );
    let direct_media = identity.source_media_sha256 == identity.effective_media_sha256
        && (gb_rtc_profile_matches(GbTasRtcHardware::Cgb, identity)
            || (identity.rtc_state == TasExternalIdentity::Absent
                && match identity.persistent_state {
                    TasExternalIdentity::Absent => {
                        identity.sync_config_sha256 == direct_gbc_tas_sync_config_sha256()
                    }
                    TasExternalIdentity::ExternalSha256(_) => {
                        identity.sync_config_sha256 == direct_gbc_battery_tas_sync_config_sha256()
                    }
                }));
    let zip_media = identity.source_media_sha256 != identity.effective_media_sha256
        && identity.sync_config_sha256 != direct_gbc_tas_sync_config_sha256()
        && identity.sync_config_sha256 != direct_gbc_battery_tas_sync_config_sha256();
    ensure!(
        identity.patches.is_empty()
            && identity.firmware == direct_gb_tas_firmware()
            && identity.devices == direct_gb_tas_devices()
            && (direct_media || zip_media),
        "TAS project is outside the direct GBC profile"
    );
    ensure!(
        identity.sensor_state == TasExternalIdentity::Absent
            && identity.cheats == TasExternalIdentity::Absent,
        "TAS project declares unsupported external state"
    );
    validate_strict_gb_start_state(project.start_state())?;
    let state =
        zeff_gb_core::save_state::inspect_current_native_tas_state_identity(project.start_state())?;
    ensure!(
        TasDigest(state.rom_sha256) == identity.effective_media_sha256
            && state.hardware_mode_preference == HardwareModePreference::ForceCgb
            && state.hardware_mode == HardwareMode::CGBNormal,
        "GBC TAS start state is outside the forced native CGB media profile"
    );
    Ok(())
}

pub(crate) fn validate_direct_gbc_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: DirectGbTasRuntimeWitness<'_>,
) -> Result<()> {
    project.validate()?;
    validate_direct_gbc_tas_project_identity(project)?;
    validate_direct_gb_tas_branch_scope(project, branch_id)?;
    let identity = project.identity();
    ensure!(
        witness.source_media_sha256 == identity.source_media_sha256
            && witness.effective_media_sha256 == identity.effective_media_sha256
            && witness.determinism_abi == identity.determinism_abi
            && witness.state_format_compatibility_id == identity.state_format_compatibility_id
            && witness.sync_config_sha256 == identity.sync_config_sha256,
        "worker execution profile does not match the GBC TAS project"
    );
    ensure!(
        TasDigest::from_bytes(witness.current_state_bytes) == witness.current_state_sha256,
        "worker current-state witness digest is inconsistent"
    );
    validate_strict_gb_start_state(witness.current_state_bytes)
}

pub(crate) fn validate_direct_gbc_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_direct_gbc_tas_runtime_inner(backend, cheats_present, false)
}

pub(crate) fn validate_direct_gbc_tas_runtime_with_project_sram(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_direct_gbc_tas_runtime_inner(backend, cheats_present, true)?;
    reject_linked_gbc_rtc(backend)
}

pub(crate) fn validate_direct_gbc_tas_runtime_with_project_rtc(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_direct_gbc_tas_runtime_inner(backend, cheats_present, true)?;
    validate_gb_rtc_runtime(backend)?;
    Ok(())
}

fn validate_direct_gbc_tas_runtime_inner(
    backend: &EmuBackend,
    cheats_present: bool,
    allow_project_sram: bool,
) -> Result<()> {
    ensure!(
        backend.system() == super::ActiveSystem::GameBoy,
        "GBC TAS execution requires a Game Boy backend"
    );
    let metadata = backend.replay_metadata();
    let provenance = backend
        .gb_tas_load_provenance()
        .context("GBC backend omitted load provenance")?;
    let effective_media_sha256 = metadata
        .rom_sha256
        .context("GBC backend omitted its effective media identity")?;
    ensure!(
        metadata.system.as_deref() == Some(super::ActiveSystem::GameBoy.code())
            && metadata.core_family.as_deref()
                == Some(format!("{:?}", zeff_emu_common::system::CoreFamily::GameBoy).as_str())
            && provenance.load.raw_source_media_sha256 == effective_media_sha256
            && provenance.load.direct_gbc_file,
        "GBC TAS execution requires one directly loaded .gbc file"
    );
    ensure!(
        !provenance.load.any_mod_enabled
            && !provenance.load.any_mod_applied
            && provenance.load.requested_hardware_mode == HardwareModePreference::ForceCgb
            && provenance.load.resolved_hardware_mode == HardwareMode::CGBNormal
            && matches!(
                provenance.current_hardware_mode,
                HardwareMode::CGBNormal | HardwareMode::CGBDouble
            )
            && provenance.current_hardware_mode_preference == HardwareModePreference::ForceCgb,
        "direct GBC TAS execution requires forced native CGB hardware"
    );
    ensure!(
        !provenance.load.external_boot_rom_used
            && !provenance.has_external_boot_rom
            && provenance.load.persistent_load
                != crate::emu_backend::gb::GbPersistentLoadOutcome::Unknown
            && provenance.load.initial_input.buttons == 0
            && provenance.load.initial_input.dpad == 0
            && provenance.load.configured_sample_rate.is_none()
            && provenance.load.initial_sample_rate == 48_000
            && provenance.current_sample_rate == 48_000,
        "direct GBC TAS execution requires internal boot, neutral input, known persistence, and 48 kHz"
    );
    ensure!(
        (allow_project_sram || !provenance.cartridge_type.is_battery_backed())
            && is_supported_direct_gb_tas_cartridge(
                provenance.cartridge_type,
                provenance.rom_size,
                provenance.ram_size,
                provenance.load.raw_source_media_len,
            )
            && provenance.is_cgb_exclusive
            && provenance.current_serial_device == GameBoySerialDevice::Disconnected,
        "direct GBC TAS runtime facts differ from the supported cartridge profile"
    );
    let battery_sram = gbc_battery_sram(backend)?;
    ensure!(
        (allow_project_sram || battery_sram.is_none())
            && (battery_sram.is_some()
                || provenance.load.persistent_load
                    == crate::emu_backend::gb::GbPersistentLoadOutcome::Absent),
        "linked GBC TAS execution requires non-battery media"
    );
    if provenance.cartridge_type.is_mbc3_with_rtc() {
        ensure!(
            allow_project_sram,
            "linked GBC TAS does not own MBC3 RTC state"
        );
        validate_gb_rtc_runtime(backend)?;
    } else {
        ensure!(
            provenance.load.rtc_time_override.is_none(),
            "non-RTC GBC TAS execution declared an RTC clock policy"
        );
    }
    let expected_firmware =
        crate::emu_backend::firmware::default_firmware_manifests_for_active_system(
            super::ActiveSystem::GameBoy,
        );
    ensure!(
        !cheats_present
            && metadata.cheat_sha256.is_none()
            && metadata.firmware == expected_firmware
            && metadata
                .firmware
                .iter()
                .all(|firmware| matches!(firmware, ReplayFirmwareManifest::Skipped { .. })),
        "direct GBC TAS execution has unsupported firmware or cheats"
    );
    Ok(())
}

pub(crate) fn validate_direct_gbc_state_for_backend(
    backend: &EmuBackend,
    state: &[u8],
    require_normal_speed: bool,
) -> Result<()> {
    validate_direct_gbc_state_for_backend_inner(backend, state, require_normal_speed, false)
}

pub(crate) fn validate_direct_gbc_state_for_backend_with_project_sram(
    backend: &EmuBackend,
    state: &[u8],
    require_normal_speed: bool,
) -> Result<()> {
    validate_direct_gbc_state_for_backend_inner(backend, state, require_normal_speed, true)?;
    reject_linked_gbc_rtc(backend)
}

pub(crate) fn validate_direct_gbc_state_for_backend_with_project_rtc(
    backend: &EmuBackend,
    state: &[u8],
    require_normal_speed: bool,
) -> Result<()> {
    validate_direct_gbc_state_for_backend_inner(backend, state, require_normal_speed, true)?;
    validate_gb_rtc_runtime(backend)?;
    Ok(())
}

fn validate_direct_gbc_state_for_backend_inner(
    backend: &EmuBackend,
    state: &[u8],
    require_normal_speed: bool,
    allow_project_sram: bool,
) -> Result<()> {
    validate_direct_gbc_tas_runtime_inner(backend, false, allow_project_sram)?;
    let gb = match backend {
        EmuBackend::Gb(gb) => gb,
        _ => bail!("GBC TAS execution requires a Game Boy backend"),
    };
    let inspection = zeff_gb_core::save_state::inspect_current_native_tas_state(&gb.emu, state)?;
    ensure!(
        inspection.hardware_mode_preference == HardwareModePreference::ForceCgb
            && matches!(
                inspection.hardware_mode,
                HardwareMode::CGBNormal | HardwareMode::CGBDouble
            )
            && (!require_normal_speed || inspection.hardware_mode == HardwareMode::CGBNormal)
            && !inspection.boot_rom_enabled
            && inspection.serial_device == GameBoySerialDevice::Disconnected,
        "GBC TAS state is outside the forced native CGB profile"
    );
    Ok(())
}

fn validate_direct_gbc_rom(bytes: &[u8]) -> Result<()> {
    let header = RomHeader::from_rom(bytes).context("direct GBC TAS media has no valid header")?;
    ensure!(
        header.is_cgb_exclusive
            && is_supported_direct_gb_tas_cartridge(
                header.cartridge_type,
                header.rom_size,
                header.ram_size,
                bytes.len(),
            ),
        "direct GBC TAS media must be CGB-exclusive and use a supported cartridge"
    );
    Ok(())
}

fn reject_linked_gbc_rtc(backend: &EmuBackend) -> Result<()> {
    ensure!(
        !backend
            .gb_tas_load_provenance()
            .is_some_and(|provenance| provenance.cartridge_type.is_mbc3_with_rtc()),
        "linked GBC TAS does not own MBC3 RTC persistence"
    );
    Ok(())
}

fn gbc_battery_sram(backend: &EmuBackend) -> Result<Option<Vec<u8>>> {
    let gb = backend.gb().context("GBC TAS backend is unavailable")?;
    Ok(gb
        .emu
        .dump_battery_sram_at_time(GB_TAS_RTC_EPOCH_UNIX_SECONDS))
}

#[cfg(test)]
mod tests;
