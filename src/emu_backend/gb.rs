use std::path::{Path, PathBuf};

use crate::audio_recorder::MidiApuSnapshot;

pub(super) fn drain_audio_samples(emu: &mut zeff_gb_core::emulator::Emulator) -> Vec<f32> {
    emu.drain_audio_samples()
}

pub(super) fn drain_audio_samples_into(
    emu: &mut zeff_gb_core::emulator::Emulator,
    buf: &mut Vec<f32>,
) {
    emu.drain_audio_samples_into(buf);
}

pub(super) fn set_sample_rate(emu: &mut zeff_gb_core::emulator::Emulator, rate: u32) {
    emu.set_sample_rate(rate);
}

pub(super) fn set_apu_sample_generation_enabled(
    emu: &mut zeff_gb_core::emulator::Emulator,
    enabled: bool,
) {
    emu.set_apu_sample_generation_enabled(enabled);
}

pub(super) fn set_apu_channel_mutes(emu: &mut zeff_gb_core::emulator::Emulator, mutes: &[bool]) {
    let arr = [
        mutes.first().copied().unwrap_or(false),
        mutes.get(1).copied().unwrap_or(false),
        mutes.get(2).copied().unwrap_or(false),
        mutes.get(3).copied().unwrap_or(false),
    ];
    emu.set_apu_channel_mutes(arr);
}

pub(super) fn set_input(
    emu: &mut zeff_gb_core::emulator::Emulator,
    buttons_pressed: u8,
    dpad_pressed: u8,
) {
    emu.set_input(buttons_pressed, dpad_pressed);
}

pub(super) fn flush_battery_sram(
    emu: &mut zeff_gb_core::emulator::Emulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    let Some(bytes) = emu.dump_battery_sram() else {
        return Ok(None);
    };
    let save_path = sram_path_for_rom(rom_path);
    crate::save_paths::write_sram_file(&save_path, &bytes)?;
    Ok(Some(save_path.display().to_string()))
}

pub(super) fn encode_state_bytes(
    emu: &zeff_gb_core::emulator::Emulator,
) -> anyhow::Result<Vec<u8>> {
    emu.encode_state_bytes()
}

pub(super) fn load_state_from_bytes(
    emu: &mut zeff_gb_core::emulator::Emulator,
    bytes: Vec<u8>,
) -> anyhow::Result<()> {
    emu.load_state_from_bytes(bytes)
}

pub(super) fn slot_path(
    emu: &zeff_gb_core::emulator::Emulator,
    slot: u8,
) -> anyhow::Result<PathBuf> {
    crate::save_paths::slot_path("gbc", "gbstate", emu.rom_hash(), slot)
}

pub(super) fn auto_save_path(emu: &zeff_gb_core::emulator::Emulator) -> Option<PathBuf> {
    Some(crate::save_paths::auto_save_path("gbc", "gbstate", emu.rom_hash()))
}

pub(super) fn load_state(
    emu: &mut zeff_gb_core::emulator::Emulator,
    slot: u8,
) -> anyhow::Result<String> {
    let path = crate::save_paths::slot_path("gbc", "gbstate", emu.rom_hash(), slot)?;
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("failed to read GB save state: {}: {e}", path.display()))?;
    emu.load_state_from_bytes(bytes)?;
    Ok(path.display().to_string())
}

pub(super) fn load_state_from_path(
    emu: &mut zeff_gb_core::emulator::Emulator,
    path: &Path,
) -> anyhow::Result<()> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("failed to read GB save state: {}: {e}", path.display()))?;
    emu.load_state_from_bytes(bytes)
}

pub(crate) fn try_load_battery_sram(
    emu: &mut zeff_gb_core::emulator::Emulator,
    rom_path: &Path,
) -> anyhow::Result<Option<String>> {
    if !emu.is_battery_backed() {
        return Ok(None);
    }
    let save_path = sram_path_for_rom(rom_path);
    if !save_path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&save_path)
        .map_err(|e| anyhow::anyhow!("failed to read GB save {}: {e}", save_path.display()))?;
    emu.load_battery_sram(&bytes)?;
    Ok(Some(save_path.display().to_string()))
}

fn sram_path_for_rom(rom_path: &Path) -> PathBuf {
    rom_path.with_extension("sav")
}

pub(super) fn rumble_active(emu: &zeff_gb_core::emulator::Emulator) -> bool {
    emu.rumble_active()
}

pub(super) fn is_mbc7(emu: &zeff_gb_core::emulator::Emulator) -> bool {
    emu.is_mbc7_cartridge()
}

pub(super) fn apu_channel_snapshot(
    emu: &zeff_gb_core::emulator::Emulator,
) -> Option<MidiApuSnapshot> {
    Some(MidiApuSnapshot::Gb(emu.apu_channel_snapshot()))
}
