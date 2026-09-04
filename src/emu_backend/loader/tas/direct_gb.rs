use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use zeff_emu_common::replay::ReplayFirmwareManifest;
use zeff_gb_core::hardware::GameBoySerialDevice;
use zeff_gb_core::hardware::ppu::DmgPalettePreset;
use zeff_gb_core::hardware::rom_header::RomHeader;
use zeff_gb_core::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use zeff_gb_core::hardware::types::{CartridgeType, RamSize, RomSize};

use super::gb_rtc::{
    GB_TAS_RTC_EPOCH_UNIX_SECONDS, GbTasRtcHardware, gb_rtc_profile_matches,
    validate_gb_rtc_runtime,
};
use super::media::read_bounded_direct_rom;
use super::{
    EmuBackend, TasCameraInput, TasDeviceIdentity, TasDigest, TasExternalIdentity,
    TasFirmwareIdentity, TasProject, tas_firmware_identity,
};

const MIN_DIRECT_GB_ROM_BYTES: u64 = 32 * 1024;
pub(super) const MAX_DIRECT_GB_ROM_BYTES: u64 = 8 * 1024 * 1024;
const GB_GAMEPAD_CONFIGURATION: &[u8] = b"zeff-tas-device-config-v1\0gb-joypad\0";
const GB_DIRECT_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gb-rom-only\0hardware=dmg\0boot=internal-post-boot\0serial=disconnected\0palette=dmg-green\0mods=disabled\0persistent-state=absent\0initial-input=neutral\0sample-rate=48000\0";
const GB_BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gb-cartridge\0hardware=dmg\0boot=internal-post-boot\0serial=disconnected\0palette=dmg-green\0mods=disabled\0persistent-state=project-owned-sram\0rtc=absent\0sensors=absent\0initial-input=neutral\0sample-rate=48000\0";
const GB_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gb-zip-member\0hardware=dmg\0boot=internal-post-boot\0serial=disconnected\0palette=dmg-green\0mods=disabled\0persistent-state=absent\0rtc=absent\0sensors=absent\0initial-input=neutral\0sample-rate=48000\0member=";
const GB_ZIP_BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gb-zip-member\0hardware=dmg\0boot=internal-post-boot\0serial=disconnected\0palette=dmg-green\0mods=disabled\0persistent-state=project-owned-sram\0rtc=absent\0sensors=absent\0initial-input=neutral\0sample-rate=48000\0member=";

pub(crate) fn direct_gb_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(GB_DIRECT_SYNC_CONFIGURATION)
}

pub(crate) fn direct_gb_battery_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(GB_BATTERY_SYNC_CONFIGURATION)
}

pub(crate) fn zip_gb_tas_sync_config_sha256(member_name: &str) -> TasDigest {
    zip_gb_tas_sync_config_sha256_for_profile(member_name, false)
}

pub(crate) fn zip_gb_battery_tas_sync_config_sha256(member_name: &str) -> TasDigest {
    zip_gb_tas_sync_config_sha256_for_profile(member_name, true)
}

fn zip_gb_tas_sync_config_sha256_for_profile(member_name: &str, battery: bool) -> TasDigest {
    let configuration = if battery {
        GB_ZIP_BATTERY_SYNC_CONFIGURATION
    } else {
        GB_ZIP_SYNC_CONFIGURATION
    };
    let mut bytes = Vec::with_capacity(configuration.len() + member_name.len());
    bytes.extend_from_slice(configuration);
    bytes.extend_from_slice(member_name.as_bytes());
    TasDigest::from_bytes(&bytes)
}

pub(super) fn direct_gb_tas_devices() -> Vec<TasDeviceIdentity> {
    vec![TasDeviceIdentity {
        port: "p1".to_owned(),
        device: "gb-joypad".to_owned(),
        configuration_sha256: TasDigest::from_bytes(GB_GAMEPAD_CONFIGURATION),
    }]
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
    let direct_media = identity.source_media_sha256 == identity.effective_media_sha256
        && (gb_rtc_profile_matches(GbTasRtcHardware::Dmg, identity)
            || (identity.rtc_state == TasExternalIdentity::Absent
                && match identity.persistent_state {
                    TasExternalIdentity::Absent => {
                        identity.sync_config_sha256 == direct_gb_tas_sync_config_sha256()
                    }
                    TasExternalIdentity::ExternalSha256(_) => {
                        identity.sync_config_sha256 == direct_gb_battery_tas_sync_config_sha256()
                    }
                }));
    let zip_media = identity.source_media_sha256 != identity.effective_media_sha256
        && identity.sync_config_sha256 != direct_gb_tas_sync_config_sha256()
        && identity.sync_config_sha256 != direct_gb_battery_tas_sync_config_sha256();
    ensure!(
        identity.patches.is_empty()
            && identity.firmware == direct_gb_tas_firmware()
            && identity.devices == direct_gb_tas_devices()
            && (direct_media || zip_media),
        "TAS project media, firmware, devices, or sync configuration is incompatible"
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
            && state.hardware_mode_preference == HardwareModePreference::ForceDmg
            && state.hardware_mode == HardwareMode::DMG,
        "Game Boy TAS start state is outside the forced DMG media profile"
    );
    Ok(())
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
    validate_direct_gb_tas_runtime_inner(backend, cheats_present, false)
}

pub(crate) fn validate_direct_gb_tas_runtime_with_project_sram(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_direct_gb_tas_runtime_inner(backend, cheats_present, true)?;
    ensure!(
        !backend
            .gb_tas_load_provenance()
            .is_some_and(|provenance| provenance.cartridge_type.is_mbc3_with_rtc()),
        "linked Game Boy TAS does not own MBC3 RTC persistence"
    );
    Ok(())
}

pub(crate) fn validate_direct_gb_tas_runtime_with_project_rtc(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_direct_gb_tas_runtime_inner(backend, cheats_present, true)?;
    validate_gb_rtc_runtime(backend)?;
    Ok(())
}

pub(super) fn validate_direct_gb_tas_runtime_inner(
    backend: &EmuBackend,
    cheats_present: bool,
    allow_project_sram: bool,
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
        provenance.load.persistent_load != crate::emu_backend::gb::GbPersistentLoadOutcome::Unknown,
        "direct Game Boy TAS execution could not establish its initial cartridge SRAM"
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
        is_supported_direct_gb_tas_cartridge(
            provenance.cartridge_type,
            provenance.rom_size,
            provenance.ram_size,
            provenance.load.raw_source_media_len,
        ) && !provenance.is_cgb_exclusive
            && provenance.dmg_palette_preset == DmgPalettePreset::default()
            && provenance.current_serial_device == GameBoySerialDevice::Disconnected,
        "direct Game Boy TAS runtime facts differ from the supported cartridge profile"
    );
    let battery_sram = gb_battery_sram(backend)?;
    ensure!(
        (allow_project_sram || battery_sram.is_none())
            && (battery_sram.is_some()
                || provenance.load.persistent_load
                    == crate::emu_backend::gb::GbPersistentLoadOutcome::Absent),
        "linked Game Boy TAS execution requires non-battery media"
    );
    if provenance.cartridge_type.is_mbc3_with_rtc() {
        ensure!(
            allow_project_sram,
            "linked Game Boy TAS does not own MBC3 RTC state"
        );
        validate_gb_rtc_runtime(backend)?;
    } else {
        ensure!(
            provenance.load.rtc_time_override.is_none(),
            "non-RTC Game Boy TAS execution declared an RTC clock policy"
        );
    }
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

pub(super) fn direct_gb_tas_firmware() -> Vec<TasFirmwareIdentity> {
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

pub(super) fn read_direct_gb_rom(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to inspect TAS source media {}", path.display()))?;
    ensure!(
        (MIN_DIRECT_GB_ROM_BYTES..=MAX_DIRECT_GB_ROM_BYTES).contains(&metadata.len()),
        "direct Game Boy TAS media must be between {MIN_DIRECT_GB_ROM_BYTES} and {MAX_DIRECT_GB_ROM_BYTES} bytes"
    );
    let expected_len =
        usize::try_from(metadata.len()).context("Game Boy TAS media is too large")?;
    read_bounded_direct_rom(
        path,
        expected_len,
        "direct Game Boy TAS media changed while it was read",
    )
}

pub(super) fn validate_direct_gb_rom(bytes: &[u8]) -> Result<RomHeader> {
    let header =
        RomHeader::from_rom(bytes).context("direct Game Boy TAS media has no valid header")?;
    ensure!(
        is_supported_direct_gb_tas_cartridge(
            header.cartridge_type,
            header.rom_size,
            header.ram_size,
            bytes.len(),
        ),
        "direct Game Boy TAS media must use a supported cartridge and match its declared ROM and RAM sizes"
    );
    ensure!(
        !header.is_cgb_exclusive,
        "direct Game Boy TAS media does not support CGB-exclusive cartridges"
    );
    Ok(header)
}

pub(crate) fn is_supported_direct_gb_tas_cartridge(
    cartridge_type: CartridgeType,
    rom_size: RomSize,
    ram_size: RamSize,
    source_len: usize,
) -> bool {
    let max_rom_bytes = match cartridge_type {
        CartridgeType::RomOnly if ram_size == RamSize::None => 32 * 1024,
        CartridgeType::RomRam if ram_size == RamSize::Kb8 => 32 * 1024,
        CartridgeType::RomRamBattery if ram_size == RamSize::Kb8 => 32 * 1024,
        CartridgeType::Mbc1 if ram_size == RamSize::None => 2 * 1024 * 1024,
        CartridgeType::Mbc1Ram
            if matches!(ram_size, RamSize::Kb8 | RamSize::Kb32)
                && !(ram_size == RamSize::Kb32 && source_len > 512 * 1024) =>
        {
            2 * 1024 * 1024
        }
        CartridgeType::Mbc1RamBattery
            if matches!(ram_size, RamSize::Kb8 | RamSize::Kb32)
                && !(ram_size == RamSize::Kb32 && source_len > 512 * 1024) =>
        {
            2 * 1024 * 1024
        }
        CartridgeType::Mbc2 if ram_size == RamSize::None => 256 * 1024,
        CartridgeType::Mbc2Battery if ram_size == RamSize::None => 256 * 1024,
        CartridgeType::Mbc3 if ram_size == RamSize::None => 2 * 1024 * 1024,
        CartridgeType::Mbc3Ram if matches!(ram_size, RamSize::Kb8 | RamSize::Kb32) => {
            2 * 1024 * 1024
        }
        CartridgeType::Mbc3RamBattery if matches!(ram_size, RamSize::Kb8 | RamSize::Kb32) => {
            2 * 1024 * 1024
        }
        CartridgeType::Mbc3TimerBattery if ram_size == RamSize::None => 2 * 1024 * 1024,
        CartridgeType::Mbc3TimerRamBattery if matches!(ram_size, RamSize::Kb8 | RamSize::Kb32) => {
            2 * 1024 * 1024
        }
        CartridgeType::Mbc5 if ram_size == RamSize::None => 8 * 1024 * 1024,
        CartridgeType::Mbc5Ram
            if matches!(ram_size, RamSize::Kb8 | RamSize::Kb32 | RamSize::Kb128) =>
        {
            8 * 1024 * 1024
        }
        CartridgeType::Mbc5RamBattery
            if matches!(ram_size, RamSize::Kb8 | RamSize::Kb32 | RamSize::Kb128) =>
        {
            8 * 1024 * 1024
        }
        _ => return false,
    };
    matches!(
        rom_size,
        RomSize::Kb32
            | RomSize::Kb64
            | RomSize::Kb128
            | RomSize::Kb256
            | RomSize::Kb512
            | RomSize::Mb1
            | RomSize::Mb2
            | RomSize::Mb4
            | RomSize::Mb8
    ) && source_len == rom_size.size_bytes()
        && source_len <= max_rom_bytes
}

pub(super) fn gb_battery_sram(backend: &EmuBackend) -> Result<Option<Vec<u8>>> {
    let gb = backend
        .gb()
        .context("Game Boy TAS backend is unavailable")?;
    Ok(gb
        .emu
        .dump_battery_sram_at_time(GB_TAS_RTC_EPOCH_UNIX_SECONDS))
}

pub(super) fn validate_strict_gb_start_state(start_state: &[u8]) -> Result<()> {
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

pub(super) fn validate_direct_gb_tas_branch_scope(
    project: &TasProject,
    branch_id: &str,
) -> Result<()> {
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
