use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use zeff_emu_common::replay::{ReplayFirmwareManifest, ReplayStartMetadata};
use zeff_gb_core::hardware::GameBoySerialDevice;
use zeff_gb_core::hardware::ppu::DmgPalettePreset;
use zeff_gb_core::hardware::rom_header::RomHeader;
use zeff_gb_core::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use zeff_gb_core::hardware::types::{CartridgeType, RamSize, RomSize};

use super::{
    BackendLoadConfig, EmuBackend, TasCameraInput, TasDeviceIdentity, TasDigest,
    TasEditorExecutionEngine, TasEditorExecutionProvider, TasExecutionSession, TasExternalIdentity,
    TasFirmwareIdentity, TasInitialBranch, TasProject, TasProjectIdentity, has_extension,
    publish_new_project, tas_firmware_identity,
};

const DIRECT_GB_ROM_BYTES: usize = 32 * 1024;
const GB_GAMEPAD_CONFIGURATION: &[u8] = b"zeff-tas-device-config-v1\0gb-joypad\0";
const GB_ROM_ONLY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gb-rom-only\0hardware=dmg\0boot=internal-post-boot\0serial=disconnected\0palette=dmg-green\0mods=disabled\0persistent-state=absent\0initial-input=neutral\0sample-rate=48000\0";

pub(crate) fn direct_gb_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(GB_ROM_ONLY_SYNC_CONFIGURATION)
}

fn direct_gb_tas_devices() -> Vec<TasDeviceIdentity> {
    vec![TasDeviceIdentity {
        port: "p1".to_owned(),
        device: "gb-joypad".to_owned(),
        configuration_sha256: TasDigest::from_bytes(GB_GAMEPAD_CONFIGURATION),
    }]
}

#[derive(Clone, Debug)]
pub(crate) struct DirectGbTasExecutionLoader {
    source_path: PathBuf,
    firmware_search_dirs: Vec<PathBuf>,
}

impl DirectGbTasExecutionLoader {
    pub(crate) fn new(source_path: PathBuf, firmware_search_dirs: Vec<PathBuf>) -> Self {
        Self {
            source_path,
            firmware_search_dirs,
        }
    }

    pub(crate) fn validate_project_branch_scope(
        project: &TasProject,
        branch_id: &str,
    ) -> Result<()> {
        validate_direct_gb_tas_branch_scope(project, branch_id)
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let (backend, source_bytes) = self.load_fresh_backend()?;
        let start_state = backend.encode_state_bytes()?;
        let identity = direct_gb_tas_identity(&backend, &source_bytes, &start_state)?;
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
        let (mut backend, source_bytes) = self.load_fresh_backend()?;
        let fresh_start_state = backend.encode_state_bytes()?;
        ensure!(
            fresh_start_state == start_state,
            "GB TAS starting state does not match the fresh direct-ROM baseline"
        );
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
        let identity = direct_gb_tas_identity(&backend, &source_bytes, start_state)?;
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

    fn load_fresh_backend(&self) -> Result<(EmuBackend, Vec<u8>)> {
        ensure!(
            has_extension(&self.source_path, "gb"),
            "TAS execution currently supports direct .gb cartridge files only"
        );
        validate_direct_gb_rom(&read_direct_gb_rom(&self.source_path)?)?;
        let backend = super::load_backend_from_rom_source(
            super::ActiveSystem::GameBoy,
            &self.source_path,
            &self.source_path,
            None,
            BackendLoadConfig {
                gb_hardware_mode_preference: HardwareModePreference::ForceDmg,
                sample_rate: None,
                apply_mods: false,
                initial_input: None,
                gb_load_battery_sram: false,
                firmware_search_dirs: self.firmware_search_dirs.clone(),
                gb_use_external_boot_rom: false,
                ..BackendLoadConfig::default()
            },
        )?
        .backend;
        let source_bytes = read_direct_gb_rom(&self.source_path)?;
        let start_state = backend.encode_state_bytes()?;
        direct_gb_tas_identity(&backend, &source_bytes, &start_state)?;
        Ok((backend, source_bytes))
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
    validate_direct_gb_tas_runtime(backend, false)?;
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
    let source_media_sha256 = TasDigest::from_bytes(source_bytes);
    ensure!(
        source_media_sha256 == effective_media_sha256,
        "direct Game Boy loader changed media bytes"
    );
    validate_strict_gb_start_state(start_state)?;
    Ok(TasProjectIdentity {
        system,
        core_family,
        determinism_abi: zeff_gb_core::save_state::TAS_DETERMINISM_ABI_ID.to_owned(),
        source_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: direct_gb_tas_firmware(),
        devices: direct_gb_tas_devices(),
        sync_config_sha256: direct_gb_tas_sync_config_sha256(),
        persistent_state: TasExternalIdentity::Absent,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id: zeff_gb_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID
            .to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    })
}

pub(crate) fn validate_direct_gb_tas_project_identity(project: &TasProject) -> Result<()> {
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
        "TAS project uses an incompatible Game Boy determinism or state format"
    );
    ensure!(
        identity.source_media_sha256 == identity.effective_media_sha256
            && identity.patches.is_empty(),
        "TAS project media is outside the direct unmodified Game Boy profile"
    );
    ensure!(
        identity.firmware == direct_gb_tas_firmware()
            && identity.devices == direct_gb_tas_devices()
            && identity.sync_config_sha256 == direct_gb_tas_sync_config_sha256(),
        "TAS project firmware, devices, or sync configuration is incompatible"
    );
    ensure!(
        identity.persistent_state == TasExternalIdentity::Absent
            && identity.rtc_state == TasExternalIdentity::Absent
            && identity.sensor_state == TasExternalIdentity::Absent
            && identity.cheats == TasExternalIdentity::Absent,
        "TAS project declares unsupported external state"
    );
    validate_strict_gb_start_state(project.start_state())
}

pub(crate) struct DirectGbTasRuntimeWitness<'a> {
    pub(crate) source_media_sha256: TasDigest,
    pub(crate) effective_media_sha256: TasDigest,
    pub(crate) current_state_bytes: &'a [u8],
    pub(crate) current_state_sha256: TasDigest,
    pub(crate) determinism_abi: &'a str,
    pub(crate) state_format_compatibility_id: &'a str,
    pub(crate) sync_config_sha256: TasDigest,
}

pub(crate) fn validate_direct_gb_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: DirectGbTasRuntimeWitness<'_>,
) -> Result<()> {
    project.validate()?;
    validate_direct_gb_tas_project_identity(project)?;
    validate_direct_gb_tas_branch_scope(project, branch_id)?;
    let identity = project.identity();
    ensure!(
        witness.source_media_sha256 == identity.source_media_sha256
            && witness.effective_media_sha256 == identity.effective_media_sha256,
        "worker media identity does not match the TAS project"
    );
    ensure!(
        witness.determinism_abi == identity.determinism_abi
            && witness.state_format_compatibility_id == identity.state_format_compatibility_id
            && witness.sync_config_sha256 == identity.sync_config_sha256,
        "worker execution profile does not match the TAS project"
    );
    ensure!(
        TasDigest::from_bytes(witness.current_state_bytes) == witness.current_state_sha256,
        "worker current-state witness digest is inconsistent"
    );
    validate_strict_gb_start_state(witness.current_state_bytes)
}

pub(crate) fn validate_direct_gb_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    ensure!(
        backend.system() == super::ActiveSystem::GameBoy,
        "TAS execution profile requires a Game Boy backend"
    );
    let metadata = backend.replay_metadata();
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::GameBoy);
    ensure!(
        metadata.system.as_deref() == Some(super::ActiveSystem::GameBoy.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        "Game Boy backend identity metadata is incompatible"
    );
    let effective_media_sha256 = metadata
        .rom_sha256
        .context("Game Boy backend omitted its effective media identity")?;
    let provenance = backend
        .gb_tas_load_provenance()
        .context("Game Boy backend omitted load provenance")?;
    ensure!(
        provenance.load.raw_source_media_sha256 == effective_media_sha256
            && provenance.load.raw_source_media_len == DIRECT_GB_ROM_BYTES
            && provenance.load.direct_gb_file,
        "Game Boy TAS execution requires one directly loaded .gb file"
    );
    ensure!(
        !provenance.load.any_mod_enabled && !provenance.load.any_mod_applied,
        "direct Game Boy TAS execution requires mods to be disabled"
    );
    ensure!(
        provenance.load.requested_hardware_mode == HardwareModePreference::ForceDmg
            && provenance.load.resolved_hardware_mode == HardwareMode::DMG
            && provenance.current_hardware_mode == HardwareMode::DMG
            && provenance.current_hardware_mode_preference == HardwareModePreference::ForceDmg,
        "direct Game Boy TAS execution requires forced DMG hardware"
    );
    ensure!(
        !provenance.load.external_boot_rom_used && !provenance.has_external_boot_rom,
        "direct Game Boy TAS execution requires internal post-boot initialization"
    );
    ensure!(
        provenance.load.persistent_load == crate::emu_backend::gb::GbPersistentLoadOutcome::Absent,
        "direct Game Boy TAS execution requires no persistent cartridge state"
    );
    ensure!(
        provenance.load.initial_input.buttons == 0
            && provenance.load.initial_input.dpad == 0
            && provenance.load.configured_sample_rate.is_none()
            && provenance.load.initial_sample_rate == 48_000
            && provenance.current_sample_rate == 48_000,
        "direct Game Boy TAS execution requires neutral input and a 48 kHz default sample rate"
    );
    ensure!(
        provenance.cartridge_type == CartridgeType::RomOnly
            && provenance.rom_size == RomSize::Kb32
            && provenance.ram_size == RamSize::None
            && !provenance.is_cgb_exclusive
            && provenance.dmg_palette_preset == DmgPalettePreset::default()
            && provenance.current_serial_device == GameBoySerialDevice::Disconnected,
        "direct Game Boy TAS runtime facts differ from the ROM-only profile"
    );
    ensure!(
        !cheats_present && metadata.cheat_sha256.is_none(),
        "Game Boy TAS execution enabled cheats"
    );
    let expected_firmware =
        crate::emu_backend::firmware::default_firmware_manifests_for_active_system(
            super::ActiveSystem::GameBoy,
        );
    ensure!(
        metadata.firmware == expected_firmware
            && metadata
                .firmware
                .iter()
                .all(|firmware| matches!(firmware, ReplayFirmwareManifest::Skipped { .. })),
        "direct Game Boy TAS execution requires the exact skipped boot-ROM manifests"
    );
    Ok(())
}

fn direct_gb_tas_firmware() -> Vec<TasFirmwareIdentity> {
    let mut firmware = crate::emu_backend::firmware::default_firmware_manifests_for_active_system(
        super::ActiveSystem::GameBoy,
    )
    .iter()
    .map(tas_firmware_identity)
    .collect::<Vec<_>>();
    firmware.sort_by(|left, right| firmware_id(left).cmp(firmware_id(right)));
    firmware
}

fn firmware_id(identity: &TasFirmwareIdentity) -> &str {
    match identity {
        TasFirmwareIdentity::External { firmware_id, .. }
        | TasFirmwareIdentity::Hle { firmware_id, .. }
        | TasFirmwareIdentity::BuiltinOpenSource { firmware_id, .. }
        | TasFirmwareIdentity::Skipped { firmware_id, .. } => firmware_id,
    }
}

fn read_direct_gb_rom(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to inspect TAS source media {}", path.display()))?;
    ensure!(
        metadata.len() == DIRECT_GB_ROM_BYTES as u64,
        "direct Game Boy TAS media must be exactly {DIRECT_GB_ROM_BYTES} bytes"
    );
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open TAS source media {}", path.display()))?;
    let mut bytes = Vec::with_capacity(DIRECT_GB_ROM_BYTES);
    file.by_ref()
        .take(DIRECT_GB_ROM_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read TAS source media {}", path.display()))?;
    ensure!(
        bytes.len() == DIRECT_GB_ROM_BYTES,
        "direct Game Boy TAS media changed while it was read"
    );
    Ok(bytes)
}

fn validate_direct_gb_rom(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() == DIRECT_GB_ROM_BYTES,
        "direct Game Boy TAS media must be exactly {DIRECT_GB_ROM_BYTES} bytes"
    );
    let header =
        RomHeader::from_rom(bytes).context("direct Game Boy TAS media has no valid header")?;
    ensure!(
        header.cartridge_type == CartridgeType::RomOnly
            && header.rom_size == RomSize::Kb32
            && header.ram_size == RamSize::None,
        "direct Game Boy TAS media must be a 32 KiB ROM-only cartridge without RAM"
    );
    ensure!(
        !header.is_cgb_exclusive,
        "direct Game Boy TAS media does not support CGB-exclusive cartridges"
    );
    Ok(())
}

fn validate_strict_gb_start_state(start_state: &[u8]) -> Result<()> {
    ensure!(
        start_state.len() >= 12 && start_state[..8] == zeff_gb_core::save_state::SAVE_STATE_MAGIC,
        "TAS starting state is not a native Game Boy save state"
    );
    let version = u32::from_le_bytes(start_state[8..12].try_into().expect("length checked"));
    ensure!(
        version == zeff_gb_core::save_state::SAVE_STATE_FORMAT_VERSION,
        "TAS execution requires native Game Boy state format {}",
        zeff_gb_core::save_state::SAVE_STATE_FORMAT_VERSION
    );
    let mut replay = start_state.to_vec();
    zeff_gb_core::save_state::project_replay_state_bytes(&mut replay)
        .context("TAS starting state failed canonical Game Boy validation")?;
    Ok(())
}

pub(crate) fn validate_direct_gb_tas_state(state: &[u8]) -> Result<()> {
    validate_strict_gb_start_state(state)
}

fn validate_direct_gb_tas_branch_scope(project: &TasProject, branch_id: &str) -> Result<()> {
    let branch = project
        .branch(branch_id)
        .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
    ensure!(
        project.replay_start() == &Default::default(),
        "direct Game Boy TAS execution does not support replay start metadata"
    );
    ensure!(
        branch.events().is_empty(),
        "direct Game Boy TAS execution does not support replay events"
    );
    for span in branch.input_spans() {
        let input = span.input;
        if input.players[0].buttons & !0x0F != 0 || input.players[0].dpad & !0x0F != 0 {
            bail!("direct Game Boy TAS execution supports only the four joypad button bits");
        }
        if input.players[1..]
            .iter()
            .any(|player| player.buttons != 0 || player.dpad != 0)
        {
            bail!("direct Game Boy TAS execution supports player 1 only");
        }
        if input.zapper.enabled
            || input.zapper.trigger
            || input.zapper.hit
            || input.zapper.screen_pos.is_some()
        {
            bail!("direct Game Boy TAS execution does not support Zapper input");
        }
        if input.tilt_x_bits != 0 || input.tilt_y_bits != 0 {
            bail!("direct Game Boy TAS execution does not support tilt input");
        }
        if input.camera != TasCameraInput::None {
            bail!("direct Game Boy TAS execution does not support camera input");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
