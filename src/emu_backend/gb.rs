use std::path::{Path, PathBuf};

use crate::audio_recorder::MidiApuSnapshot;

pub(super) fn drain_audio_samples(emu: &mut zeff_gb_core::emulator::Emulator) -> Vec<f32> {
    emu.bus.apu_drain_samples()
}

pub(super) fn drain_audio_samples_into(
    emu: &mut zeff_gb_core::emulator::Emulator,
    buf: &mut Vec<f32>,
) {
    emu.bus.apu_drain_samples_into(buf);
}

pub(super) fn set_sample_rate(emu: &mut zeff_gb_core::emulator::Emulator, rate: u32) {
    emu.bus.set_apu_sample_rate(rate);
}

pub(super) fn set_apu_sample_generation_enabled(
    emu: &mut zeff_gb_core::emulator::Emulator,
    enabled: bool,
) {
    emu.bus.set_apu_sample_generation_enabled(enabled);
}

pub(super) fn set_apu_channel_mutes(emu: &mut zeff_gb_core::emulator::Emulator, mutes: [bool; 4]) {
    emu.bus.set_apu_channel_mutes(mutes);
}

pub(super) fn set_input(
    emu: &mut zeff_gb_core::emulator::Emulator,
    buttons_pressed: u8,
    dpad_pressed: u8,
) {
    if emu
        .bus
        .apply_joypad_pressed_masks(buttons_pressed, dpad_pressed)
    {
        emu.bus.if_reg |= 0x10;
    }
}

pub(super) fn flush_battery_sram(
    emu: &mut zeff_gb_core::emulator::Emulator,
) -> anyhow::Result<Option<String>> {
    emu.flush_battery_sram()
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
    zeff_gb_core::save_state::slot_path(emu.rom_hash, slot)
}

pub(super) fn auto_save_path(emu: &zeff_gb_core::emulator::Emulator) -> Option<PathBuf> {
    Some(zeff_gb_core::save_state::auto_save_path(emu.rom_hash))
}

pub(super) fn load_state(
    emu: &mut zeff_gb_core::emulator::Emulator,
    slot: u8,
) -> anyhow::Result<String> {
    emu.load_state(slot)
}

pub(super) fn load_state_from_path(
    emu: &mut zeff_gb_core::emulator::Emulator,
    path: &Path,
) -> anyhow::Result<()> {
    emu.load_state_from_path(path)
}

pub(super) fn rumble_active(emu: &zeff_gb_core::emulator::Emulator) -> bool {
    emu.bus.cartridge.rumble_active()
}

pub(super) fn is_mbc7(emu: &zeff_gb_core::emulator::Emulator) -> bool {
    emu.is_mbc7_cartridge()
}

pub(super) fn apu_channel_snapshot(
    emu: &zeff_gb_core::emulator::Emulator,
) -> Option<MidiApuSnapshot> {
    Some(MidiApuSnapshot::Gb(emu.bus.apu_channel_snapshot()))
}

